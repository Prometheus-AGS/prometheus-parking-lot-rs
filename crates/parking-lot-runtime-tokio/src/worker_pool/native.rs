//! Native implementation of `WorkerPool` using OS threads.
//!
//! This implementation spawns dedicated OS threads that each have their own
//! single-threaded tokio runtime. This ensures CPU/GPU-bound work does not
//! block the main async runtime.
//!
//! # Design Principles
//!
//! - **One dispatcher**: admission goes through the core
//!   [`Dispatcher`](prometheus_parking_lot_core::dispatch::Dispatcher) shared
//!   with `ResourcePool`. It enforces `max_units` (audit C1: the 0.2 pool
//!   ignored it), serves parked tasks by priority then submission order, and
//!   bounds admitted tasks by the worker count, so an admitted task always has
//!   a worker and `max_queue_depth` bounds exactly the tasks waiting.
//! - **Bounded execution**: every execution runs under a budget (the task's
//!   deadline or the pool's default timeout, whichever is nearer); a task
//!   still running at its budget is cancelled, its units are released and its
//!   result is [`TaskError::Timeout`].
//! - **No polling**: workers block on a channel of admitted tasks; results use
//!   a Condvar.
//! - **Clean shutdown**: dropping the sender unblocks workers naturally;
//!   parked tasks are failed with [`TaskError::Cancelled`] rather than dropped
//!   silently.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use parking_lot::{Condvar, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::dispatch::{
    Dispatched, Dispatcher, Grant, PoolCounters, WorkerTask, generate_mailbox_key,
    mailbox_key_to_string,
};
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{
    Clock, MailboxKey, PoolStats, SchedulerError, SystemClock, TaskError, TaskMetadata,
    WorkerExecutor,
};

/// Pool name used in errors (pools are not named yet).
const POOL_NAME: &str = "worker_pool";

/// What a worker stores for one task: the executor's value or its error.
type TaskOutcome<R> = Result<R, TaskError>;

/// The error for a result slot that does not exist (never created, already
/// retrieved, or removed after a timeout).
fn result_not_found(key: &MailboxKey) -> SchedulerError {
    SchedulerError::ResultNotFound {
        key: Some(mailbox_key_to_string(key)),
    }
}

/// The error for a submission to a pool that is shutting down.
fn pool_shutdown() -> SchedulerError {
    SchedulerError::PoolShutdown {
        pool: POOL_NAME.into(),
    }
}

/// The error for a result that did not arrive within the caller's wait.
const fn wait_timed_out() -> SchedulerError {
    SchedulerError::Timeout { task_id: None }
}

/// Result entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultState {
    /// Waiting for result.
    Pending,
    /// Result is ready.
    Ready,
    /// The slot was removed/cancelled before a result arrived; any waiter must
    /// wake and stop waiting. This prevents a waiter from parking forever if the
    /// producing worker never stores a result (e.g. it was cancelled or the
    /// caller timed out and removed the slot first).
    Cancelled,
}

/// Result storage entry with Condvar-based notification.
struct ResultEntry<R> {
    /// The result value (once available).
    result: Option<R>,
    /// State of this entry.
    state: ResultState,
}

/// Result storage for the worker pool using Condvar for efficient waiting.
///
/// Design:
/// - `RwLock` for the entry map (read-heavy, write on create/remove)
/// - Per-entry Mutex + Condvar for waiting (lock only when blocking wait needed)
/// - Lock-free check via state atomic would be ideal but Condvar needs Mutex
struct ResultStorage<R> {
    /// Map from mailbox key to (entry, condvar) pair.
    /// The Condvar is used for blocking wait, paired with entry's mutex.
    #[expect(
        clippy::type_complexity,
        reason = "private storage map; the worker-pool rework (cr-19/cr-29) replaces it"
    )]
    entries: RwLock<HashMap<String, Arc<(Mutex<ResultEntry<R>>, Condvar)>>>,
}

impl<R> ResultStorage<R> {
    fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    /// Create a slot for a result.
    fn create_slot(&self, key: &MailboxKey) {
        let key_str = mailbox_key_to_string(key);

        let entry = ResultEntry {
            result: None,
            state: ResultState::Pending,
        };

        let mut entries = self.entries.write();
        entries.insert(key_str, Arc::new((Mutex::new(entry), Condvar::new())));
    }

    /// Store a result and notify any waiters.
    /// This is lock-free for the map lookup, only locks the entry briefly.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "narrowing lock scopes changes concurrent behaviour; the worker-pool rework (cr-19/cr-29) owns it"
    )]
    fn store(&self, key: &MailboxKey, result: R) {
        let key_str = mailbox_key_to_string(key);

        // Read lock on map (fast, concurrent reads allowed)
        let entries = self.entries.read();
        if let Some(entry_pair) = entries.get(&key_str) {
            let (entry_mutex, condvar) = entry_pair.as_ref();
            // Brief lock on entry
            let mut entry = entry_mutex.lock();
            entry.result = Some(result);
            entry.state = ResultState::Ready;
            // Notify ALL waiters (there should only be one, but be safe)
            condvar.notify_all();
        }
    }

    /// Try to retrieve a result immediately (non-blocking).
    #[expect(
        clippy::significant_drop_tightening,
        reason = "narrowing lock scopes changes concurrent behaviour; the worker-pool rework (cr-19/cr-29) owns it"
    )]
    fn try_retrieve(&self, key: &MailboxKey) -> Option<R> {
        let key_str = mailbox_key_to_string(key);

        let entries = self.entries.read();
        if let Some(entry_pair) = entries.get(&key_str) {
            let (entry_mutex, _) = entry_pair.as_ref();
            let mut entry = entry_mutex.lock();
            if entry.state == ResultState::Ready {
                return entry.result.take();
            }
        }
        None
    }

    /// Wait for a result with timeout (blocking).
    /// Uses Condvar for efficient waiting - NO POLLING.
    fn wait_for_result(&self, key: &MailboxKey, timeout: Duration) -> Result<R, SchedulerError> {
        // Fix the deadline before touching any lock so lock contention counts
        // against the caller's budget rather than extending it.
        let deadline = Instant::now() + timeout;

        // Get the entry pair (need to hold Arc while waiting)
        let entry_pair = {
            let entries = self.entries.read();
            entries.get(&mailbox_key_to_string(key)).cloned()
        };

        let Some(entry_pair) = entry_pair else {
            return Err(result_not_found(key));
        };

        wait_on_entry(&entry_pair, deadline)
    }

    /// Remove a result entry entirely.
    ///
    /// Any task still parked on this entry's `Condvar` is woken before we drop
    /// our reference: we mark the entry `Cancelled` (unless a result already
    /// arrived) and `notify_all`. Because a waiter holds its own `Arc` to the
    /// entry, removing it from the map is not enough to wake it — without this
    /// notify a waiter whose result never gets stored would block forever and
    /// wedge the runtime on shutdown. See `retrieve_async`.
    fn remove(&self, key: &MailboxKey) -> Option<R> {
        let key_str = mailbox_key_to_string(key);

        // Drop the map entry under the write lock, then deal with the entry
        // itself outside the map lock.
        let entry_pair = {
            let mut entries = self.entries.write();
            entries.remove(&key_str)
        };

        entry_pair.and_then(|entry_pair| {
            let (entry_mutex, condvar) = entry_pair.as_ref();
            // Mutate state under the lock, then release it before notifying:
            // the state change is published under the lock (no lost wakeup),
            // and notifying after unlock avoids the woken waiter immediately
            // re-blocking on a still-held mutex.
            let taken = {
                let mut entry = entry_mutex.lock();
                let taken = entry.result.take();
                if entry.state != ResultState::Ready {
                    entry.state = ResultState::Cancelled;
                }
                taken
            };
            // Wake any waiter so it can observe the terminal state and exit.
            condvar.notify_all();
            taken
        })
    }

    /// Get entry for async waiting (returns clone of Arc).
    fn get_entry(&self, key: &MailboxKey) -> Option<Arc<(Mutex<ResultEntry<R>>, Condvar)>> {
        let key_str = mailbox_key_to_string(key);
        let entries = self.entries.read();
        entries.get(&key_str).cloned()
    }
}

/// Block on one result slot until a result is stored or `deadline` passes.
///
/// This is the single place that maps a slot's terminal state to the
/// caller-visible outcome, shared by `retrieve` and `retrieve_async`:
///
/// - `Ready` with a value → `Ok(result)`.
/// - `Ready` but the value was already taken (a concurrent retriever consumed
///   it) → `SchedulerError::ResultNotFound`.
/// - still `Pending` at the deadline, or `Cancelled` (the slot was reclaimed
///   before a result arrived) → `SchedulerError::Timeout`.
///
/// The slot value is the task's outcome (`Result<_, TaskError>`); callers map
/// an executor error to `SchedulerError::Task`.
///
/// The wait loops on the slot state against an absolute deadline, so a spurious
/// Condvar wakeup neither returns early nor restarts the full timeout. A result
/// stored at the same instant the deadline fires is still delivered, because
/// the state is re-read under the reacquired lock after the wait.
fn wait_on_entry<R>(
    entry_pair: &(Mutex<ResultEntry<R>>, Condvar),
    deadline: Instant,
) -> Result<R, SchedulerError> {
    let (entry_mutex, condvar) = entry_pair;
    let mut entry = entry_mutex.lock();
    while entry.state == ResultState::Pending {
        if condvar.wait_until(&mut entry, deadline).timed_out() {
            break;
        }
    }
    match entry.state {
        ResultState::Ready => entry
            .result
            .take()
            .ok_or(SchedulerError::ResultNotFound { key: None }),
        ResultState::Pending | ResultState::Cancelled => Err(wait_timed_out()),
    }
}

/// An admitted task on its way to a worker.
type Admitted<P> = Grant<WorkerTask<P>>;

/// State shared by the pool handle and its worker threads.
struct Shared<P, R, E> {
    /// Pool configuration.
    config: WorkerPoolConfig,
    /// Admission, the parked queue and the semaphore.
    dispatcher: Dispatcher<WorkerTask<P>>,
    /// Result storage with Condvar-based notification.
    results: ResultStorage<TaskOutcome<R>>,
    /// Submitted/completed/failed counters (running and parked come from the
    /// dispatcher).
    counters: PoolCounters,
    /// Admitted tasks to the workers. `None` once the pool shuts down; the
    /// workers exit when it is gone and the channel is empty.
    ready_tx: Mutex<Option<Sender<Admitted<P>>>>,
    /// Time source for deadlines and budgets.
    clock: RwLock<Arc<dyn Clock>>,
    /// Shutdown flag.
    shutdown: AtomicBool,
    /// The executor; each worker runs a clone.
    executor: E,
}

impl<P, R, E> Shared<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    fn now_ms(&self) -> u128 {
        self.clock.read().now_ms()
    }

    /// Resolve the execution budget for one task: the task's deadline when it
    /// is nearer than the pool's default timeout, otherwise the default. No
    /// path is unbounded; an already-passed deadline gives a zero budget.
    #[cfg(not(parking_lot_negative_control = "s-4"))]
    fn budget(&self, meta: &TaskMetadata) -> Duration {
        let default = self.config.default_timeout();
        let Some(deadline) = meta.deadline_ms else {
            return default;
        };
        let remaining = deadline.saturating_sub(self.now_ms());
        Duration::from_millis(u64::try_from(remaining).unwrap_or(u64::MAX)).min(default)
    }

    /// Negative control S-4: the budget is not applied (effectively
    /// unbounded), so the `budget_` wedged-executor tests must fail.
    #[cfg(parking_lot_negative_control = "s-4")]
    fn budget(&self, meta: &TaskMetadata) -> Duration {
        let _ = (meta, &self.config);
        Duration::from_secs(u64::from(u32::MAX))
    }

    /// Record a task's outcome in its result slot and count it.
    fn finish(&self, key: &MailboxKey, outcome: TaskOutcome<R>) {
        let counter = if outcome.is_ok() {
            &self.counters.completed_tasks
        } else {
            &self.counters.failed_tasks
        };
        counter.fetch_add(1, Ordering::Relaxed);
        self.results.store(key, outcome);
    }

    /// Hand every admitted task to the workers and fail every task found
    /// expired at the head of the queue.
    ///
    /// A task admitted after shutdown took the sender cannot run: it is
    /// failed with [`TaskError::Cancelled`] (its permit is released by drop;
    /// the parked queue was already emptied by `shutdown`).
    fn launch(&self, dispatched: Dispatched<WorkerTask<P>>) {
        let Dispatched { started, expired } = dispatched;
        for task in expired {
            warn!(
                task_id = task.meta.id,
                "queued task expired before it could start"
            );
            self.finish(&task.mailbox_key, Err(TaskError::DeadlineExpired));
        }
        if started.is_empty() {
            return;
        }
        // A clone sent after `shutdown` took the original still delivers: the
        // worker then sees the shutdown flag and cancels the task.
        let ready_tx = self.ready_tx.lock().clone();
        for admitted in started {
            let refused = match ready_tx.as_ref() {
                Some(tx) => tx.send(admitted).err().map(|error| error.0),
                None => Some(admitted),
            };
            if let Some(Grant { item: task, permit }) = refused {
                drop(permit);
                self.finish(&task.mailbox_key, Err(shutdown_cancelled()));
            }
        }
    }

    /// Fail every parked task (shutdown): nothing parked is dropped silently.
    fn cancel_parked(&self) {
        for task in self.dispatcher.take_parked() {
            self.finish(&task.mailbox_key, Err(shutdown_cancelled()));
        }
    }

    /// Run one admitted task on the calling worker under its budget, then
    /// return its permit and hand on whatever that admits.
    fn execute(&self, runtime: &tokio::runtime::Runtime, admitted: Admitted<P>) {
        let Grant { item: task, permit } = admitted;
        let WorkerTask {
            payload,
            meta,
            mailbox_key,
        } = task;
        let task_id = meta.id;

        let outcome = if self.shutdown.load(Ordering::Acquire) {
            // Acquire pairs with the Release in `shutdown()`: the pool is
            // shutting down, so the task is failed rather than started.
            Err(shutdown_cancelled())
        } else {
            let budget = self.budget(&meta);
            debug!(task_id, ?budget, "worker executing task");
            // On elapse the executor future is dropped, which cancels it at
            // its next await point. The timer is created inside `block_on`:
            // it needs the worker runtime's time driver in context.
            runtime
                .block_on(async {
                    tokio::time::timeout(budget, self.executor.execute(payload, meta)).await
                })
                .unwrap_or_else(|_elapsed| {
                    warn!(
                        task_id,
                        ?budget,
                        "task exceeded its budget and was cancelled"
                    );
                    Err(TaskError::Timeout { budget })
                })
        };

        // Return the units before publishing the outcome, so a caller that
        // has retrieved the result already sees them released.
        let woken = self.dispatcher.complete(permit, self.now_ms());
        self.finish(&mailbox_key, outcome);
        self.launch(woken);
    }
}

/// The error for a task that will not run because the pool shut down.
fn shutdown_cancelled() -> TaskError {
    TaskError::Cancelled {
        reason: "worker pool shut down".into(),
    }
}

/// Worker pool with dedicated OS threads for CPU/GPU-bound work.
///
/// Each worker thread has its own single-threaded tokio runtime, ensuring
/// that executor work does not block the main async runtime.
///
/// # Design
///
/// - **Capacity**: a task runs only while the units of all running tasks,
///   including it, fit in `max_units`, and at most `worker_count` tasks run.
///   Others wait in a priority queue (at most `max_queue_depth`), highest
///   priority first, FIFO within a priority.
/// - **Budgets**: each execution is bounded by the task's deadline or
///   `default_timeout_ms`, whichever is nearer.
/// - **No polling**: Workers block on channel recv; results use Condvar
/// - **Clean shutdown**: Dropping sender naturally unblocks all workers
pub struct WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// State shared with the workers.
    shared: Arc<Shared<P, R, E>>,

    /// Worker thread handles.
    workers: Mutex<Vec<JoinHandle<()>>>,

    /// Task ID counter (lock-free atomic).
    task_id_counter: AtomicU64,
}

impl<P, R, E> WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// Create a new worker pool with the given configuration and executor.
    ///
    /// This spawns `config.worker_count` OS threads, each with its own
    /// single-threaded tokio runtime for executing tasks.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::InvalidConfig`] if the configuration is invalid.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "public constructor takes the config by value; changing it is an API change"
    )]
    pub fn new(config: WorkerPoolConfig, executor: E) -> Result<Self, SchedulerError> {
        config.validate().map_err(SchedulerError::invalid_config)?;
        let worker_permits = u32::try_from(config.worker_count).unwrap_or(u32::MAX);

        let (ready_tx, ready_rx) = unbounded::<Admitted<P>>();
        let shared = Arc::new(Shared {
            dispatcher: Dispatcher::in_memory(
                POOL_NAME,
                UnitSemaphore::with_max_permits(config.max_units, worker_permits),
                config.max_queue_depth,
            ),
            config: config.clone(),
            results: ResultStorage::new(),
            counters: PoolCounters::default(),
            ready_tx: Mutex::new(Some(ready_tx)),
            clock: RwLock::new(Arc::new(SystemClock::new())),
            shutdown: AtomicBool::new(false),
            executor,
        });

        let workers = (0..config.worker_count)
            .map(|worker_id| {
                spawn_worker(
                    worker_id,
                    ready_rx.clone(),
                    Arc::clone(&shared),
                    config.thread_stack_size,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                SchedulerError::internal(format!("failed to spawn worker thread: {error}"))
            })?;

        info!(
            worker_count = config.worker_count,
            max_units = config.max_units,
            max_queue_depth = config.max_queue_depth,
            "WorkerPool initialized with dedicated OS threads (no-polling design)"
        );

        Ok(Self {
            shared,
            workers: Mutex::new(workers),
            task_id_counter: AtomicU64::new(0),
        })
    }

    /// Read time from `clock` instead of the system clock: deadline checks
    /// at submission and at the head of the queue, and execution budgets.
    ///
    /// Pass a [`TestClock`](prometheus_parking_lot_core::TestClock) clone to
    /// drive deadlines from a test.
    #[must_use]
    pub fn with_clock(self, clock: impl Clock + 'static) -> Self {
        *self.shared.clock.write() = Arc::new(clock);
        self
    }

    /// Submit a task asynchronously.
    ///
    /// This method can be called from an async context and will not block.
    ///
    /// # Returns
    ///
    /// Returns a `MailboxKey` that can be used to retrieve the result.
    ///
    /// # Errors
    ///
    /// As [`submit`](Self::submit).
    #[expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "public async API kept for callers; admission is non-blocking (the trait-impl variant misfires on an inherent method)"
    )]
    pub async fn submit_async(
        &self,
        payload: P,
        meta: TaskMetadata,
    ) -> Result<MailboxKey, SchedulerError> {
        self.submit(payload, meta)
    }

    /// Submit a task (non-blocking).
    ///
    /// The task starts at once when nothing is parked, its cost fits the free
    /// units and a worker is free; otherwise it waits in the priority queue.
    ///
    /// # Returns
    ///
    /// Returns a `MailboxKey` that can be used to retrieve the result.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::PoolShutdown`] if the pool has been shut down
    /// - [`SchedulerError::DeadlineExpired`] if the task's deadline has already
    ///   passed by the pool's clock
    /// - [`SchedulerError::CapacityExceeded`] if the task costs more units than
    ///   the pool has in total (it could never run)
    /// - [`SchedulerError::QueueFull`] if the task must wait and
    ///   `max_queue_depth` tasks are already waiting
    pub fn submit(&self, payload: P, meta: TaskMetadata) -> Result<MailboxKey, SchedulerError> {
        let shared = &self.shared;
        // Acquire pairs with the Release in `shutdown()`.
        if shared.shutdown.load(Ordering::Acquire) {
            return Err(pool_shutdown());
        }
        let now_ms = shared.now_ms();
        if meta.deadline_ms.is_some_and(|deadline| now_ms > deadline) {
            return Err(SchedulerError::DeadlineExpired { task_id: meta.id });
        }

        // Relaxed: the counter only needs unique values.
        let task_id = self.task_id_counter.fetch_add(1, Ordering::Relaxed);
        let mailbox_key = generate_mailbox_key(task_id);
        shared.results.create_slot(&mailbox_key);

        let task = WorkerTask {
            payload,
            meta,
            mailbox_key: mailbox_key.clone(),
        };
        let admission = match shared.dispatcher.submit(task, now_ms) {
            Ok(admission) => admission,
            Err(error) => {
                shared.results.remove(&mailbox_key);
                warn!(%error, "worker pool rejected task");
                return Err(error);
            }
        };
        shared
            .counters
            .submitted_tasks
            .fetch_add(1, Ordering::Relaxed);
        debug!(
            task_id,
            started = admission.started,
            "task submitted to worker pool"
        );
        shared.launch(admission.dispatched);

        // A shutdown that raced this submission may have emptied the parked
        // queue before this task was parked; fail anything parked since.
        // Acquire pairs with the Release in `shutdown()`.
        if shared.shutdown.load(Ordering::Acquire) {
            shared.cancel_parked();
        }
        Ok(mailbox_key)
    }

    /// Retrieve a result asynchronously with timeout.
    ///
    /// This method waits for the result to become available or times out.
    /// Uses tokio's async timing - no polling.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::Task`] carrying the [`TaskError`] when the executor
    ///   failed, ran past its budget ([`TaskError::Timeout`]), expired while
    ///   queued or was cancelled at shutdown
    /// - [`SchedulerError::Timeout`] if the result is not available within the
    ///   timeout
    /// - [`SchedulerError::ResultNotFound`] if the mailbox key is invalid
    pub async fn retrieve_async(
        &self,
        key: &MailboxKey,
        timeout: Duration,
    ) -> Result<R, SchedulerError> {
        let results = &self.shared.results;
        // First, try immediate retrieval (fast path)
        if let Some(outcome) = results.try_retrieve(key) {
            results.remove(key);
            return outcome.map_err(SchedulerError::Task);
        }

        // Get entry for waiting
        let entry_pair = results
            .get_entry(key)
            .ok_or_else(|| result_not_found(key))?;

        // One absolute deadline shared by both waits below. The blocking wait
        // is bounded by it (so the `spawn_blocking` thread cannot park forever
        // and wedge runtime shutdown if `store` never fires), and the outer
        // tokio timeout covers the case where the blocking pool is saturated
        // and the closure has not started yet. Whichever of the two fires
        // first, an elapsed deadline is reported as `SchedulerError::Timeout`:
        // `wait_on_entry` maps a still-pending slot to `Timeout`, never to
        // `ResultNotFound`.
        let deadline = Instant::now() + timeout;
        let waited = tokio::time::timeout(
            timeout,
            tokio::task::spawn_blocking(move || wait_on_entry(&entry_pair, deadline)),
        )
        .await;

        // Clean up the entry. This also wakes a still-parked blocking waiter
        // (outer timeout fired first) via `remove`'s notify.
        results.remove(key);

        match waited {
            Ok(Ok(outcome)) => outcome?.map_err(SchedulerError::Task),
            Ok(Err(join_error)) => Err(SchedulerError::internal(format!(
                "result wait task failed: {join_error}"
            ))),
            Err(_elapsed) => Err(wait_timed_out()),
        }
    }

    /// Retrieve a result (blocking API) with timeout.
    ///
    /// This method blocks the current thread until the result is available
    /// or the timeout expires. Uses Condvar for efficient waiting - NO POLLING.
    ///
    /// # Errors
    ///
    /// As [`retrieve_async`](Self::retrieve_async).
    pub fn retrieve(&self, key: &MailboxKey, timeout: Duration) -> Result<R, SchedulerError> {
        let results = &self.shared.results;
        let outcome = results.wait_for_result(key, timeout);
        // Clean up entry on any outcome
        results.remove(key);
        outcome?.map_err(SchedulerError::Task)
    }

    /// Get current pool statistics.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let shared = &self.shared;
        let mut stats = shared
            .counters
            .snapshot(shared.config.worker_count, shared.config.max_units);
        stats.used_units = shared.dispatcher.units_in_use();
        stats.active_tasks = u64::from(shared.dispatcher.running());
        stats.queued_tasks = u64::try_from(shared.dispatcher.parked()).unwrap_or(u64::MAX);
        stats
    }

    /// Shut down the pool gracefully with timeout.
    ///
    /// New submissions are refused, tasks still waiting in the queue are
    /// failed with [`TaskError::Cancelled`], admitted tasks not yet started
    /// are failed the same way, and running tasks finish (within their
    /// budgets). Workers are then joined with a 2-second bound each; a worker
    /// that does not exit in time is detached so shutdown never hangs.
    pub fn shutdown(&self) {
        let shared = &self.shared;
        // AcqRel: Release publishes the flag to the Acquire loads in
        // `submit`/`execute`; Acquire makes a second caller see the first.
        if shared.shutdown.swap(true, Ordering::AcqRel) {
            return; // Already shut down
        }

        info!("Shutting down worker pool");

        // Drop the sender to unblock all workers waiting on recv()
        drop(shared.ready_tx.lock().take());
        shared.cancel_parked();

        // Join workers with timeout
        let workers: Vec<_> = self.workers.lock().drain(..).collect();
        let worker_count = workers.len();

        for (idx, worker) in workers.into_iter().enumerate() {
            // Try to join with timeout using a helper thread
            let (tx, rx) = std::sync::mpsc::channel();
            let join_thread = thread::spawn(move || {
                let result = worker.join();
                let _ = tx.send(result.is_ok());
            });

            // Wait up to 2 seconds for this worker to exit.
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(true) => {
                    debug!(worker_id = idx, "Worker joined successfully");
                    // Helper has already sent its result, so it is about to
                    // finish — joining it returns immediately.
                    let _ = join_thread.join();
                }
                Ok(false) => {
                    warn!(worker_id = idx, "Worker panicked");
                    let _ = join_thread.join();
                }
                Err(_) => {
                    warn!(
                        worker_id = idx,
                        "Worker did not exit within timeout - detaching"
                    );
                    // The helper is still blocked on `worker.join()`; joining it
                    // here would block until the wedged worker exits, defeating
                    // the timeout. Detach instead so shutdown() always returns
                    // within its per-worker bound. The OS reclaims the thread.
                    drop(join_thread);
                }
            }
        }

        info!(
            worker_count = worker_count,
            "Worker pool shut down complete"
        );
    }
}

impl<P, R, E> Drop for WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    fn drop(&mut self) {
        // Signal shutdown but DON'T join workers in Drop
        // This prevents test hangs when pools are dropped with tasks still running
        if !self.shared.shutdown.swap(true, Ordering::AcqRel) {
            // Drop the sender to unblock waiting workers
            drop(self.shared.ready_tx.lock().take());
            self.shared.cancel_parked();

            // DON'T join workers here - let OS clean up threads
            // Explicit shutdown() is required for graceful cleanup
            debug!("WorkerPool dropped without explicit shutdown - workers will be detached");
        }
    }
}

/// Spawn a worker thread.
fn spawn_worker<P, R, E>(
    worker_id: usize,
    ready_rx: Receiver<Admitted<P>>,
    shared: Arc<Shared<P, R, E>>,
    stack_size: usize,
) -> std::io::Result<JoinHandle<()>>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    thread::Builder::new()
        .name(format!("pl-worker-{worker_id}"))
        .stack_size(stack_size)
        .spawn(move || {
            debug!(worker_id = worker_id, "Worker thread started");

            // Each worker has its own single-threaded tokio runtime
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    error!(
                        worker_id = worker_id,
                        error = %e,
                        "Failed to create worker runtime"
                    );
                    return;
                }
            };

            // Worker loop - blocking recv, NO POLLING. When the sender is
            // dropped (shutdown) and the channel is empty, recv returns Err
            // and the worker exits.
            while let Ok(admitted) = ready_rx.recv() {
                shared.execute(&runtime, admitted);
            }

            debug!(worker_id = worker_id, "Worker thread exiting");
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use prometheus_parking_lot_core::{ResourceCost, ResourceKind};
    use std::sync::atomic::AtomicUsize;

    /// Test executor that records which thread it runs on.
    #[derive(Clone)]
    struct TestExecutor {
        execution_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl WorkerExecutor<String, String> for TestExecutor {
        async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
            self.execution_count.fetch_add(1, Ordering::Relaxed);
            // Simulate some work
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(format!("Result: {payload}"))
        }
    }

    fn make_meta(id: u64) -> TaskMetadata {
        TaskMetadata {
            id,
            mailbox: None,
            priority: prometheus_parking_lot_core::Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
        }
    }

    #[tokio::test]
    async fn test_worker_pool_basic() {
        let executor = TestExecutor {
            execution_count: Arc::new(AtomicUsize::new(0)),
        };

        let config = WorkerPoolConfig::new()
            .with_worker_count(2)
            .with_max_queue_depth(10);

        let pool = WorkerPool::new(config, executor.clone()).unwrap();

        // Submit a task
        let key = pool
            .submit_async("hello".to_string(), make_meta(1))
            .await
            .unwrap();

        // Retrieve result
        let result = pool
            .retrieve_async(&key, Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(result, "Result: hello");

        // Check execution count
        assert_eq!(executor.execution_count.load(Ordering::Relaxed), 1);

        // Check stats
        let stats = pool.stats();
        assert_eq!(stats.completed_tasks, 1);
        assert_eq!(stats.submitted_tasks, 1);
    }

    #[tokio::test]
    async fn test_worker_pool_multiple_tasks() {
        let executor = TestExecutor {
            execution_count: Arc::new(AtomicUsize::new(0)),
        };

        let config = WorkerPoolConfig::new()
            .with_worker_count(4)
            .with_max_queue_depth(100);

        let pool = WorkerPool::new(config, executor.clone()).unwrap();

        // Submit multiple tasks
        let mut keys = Vec::new();
        for i in 0..10 {
            let key = pool
                .submit_async(format!("task-{i}"), make_meta(i))
                .await
                .unwrap();
            keys.push(key);
        }

        // Retrieve all results
        for (i, key) in keys.iter().enumerate() {
            let result = pool
                .retrieve_async(key, Duration::from_secs(10))
                .await
                .unwrap();
            assert_eq!(result, format!("Result: task-{i}"));
        }

        // Check execution count
        assert_eq!(executor.execution_count.load(Ordering::Relaxed), 10);
    }

    /// Executor that blocks far longer than the shutdown join bound, to prove
    /// `shutdown()` detaches a wedged worker instead of hanging on it (CR-02).
    #[derive(Clone)]
    struct WedgedExecutor;

    #[async_trait]
    impl WorkerExecutor<String, String> for WedgedExecutor {
        async fn execute(
            &self,
            _payload: String,
            _meta: TaskMetadata,
        ) -> Result<String, TaskError> {
            // Much longer than shutdown's 2s-per-worker join bound.
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok("unreachable".to_string())
        }
    }

    #[test]
    fn test_shutdown_returns_within_bound_with_wedged_worker() {
        let config = WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_queue_depth(10);
        let pool = WorkerPool::new(config, WedgedExecutor).unwrap();

        // Occupy the single worker with a 30s task.
        let _ = pool.submit("stuck".to_string(), make_meta(1)).unwrap();
        // Give the worker a moment to pick up the task.
        thread::sleep(Duration::from_millis(100));

        let start = std::time::Instant::now();
        pool.shutdown();
        let elapsed = start.elapsed();

        // Must return within the per-worker bound (2s) plus slack — NOT 30s.
        assert!(
            elapsed < Duration::from_secs(5),
            "shutdown() blocked on a wedged worker: took {elapsed:?}"
        );
    }

    #[test]
    fn test_worker_pool_blocking_api() {
        let executor = TestExecutor {
            execution_count: Arc::new(AtomicUsize::new(0)),
        };

        let config = WorkerPoolConfig::new()
            .with_worker_count(2)
            .with_max_queue_depth(10);

        let pool = WorkerPool::new(config, executor).unwrap();

        // Submit using blocking API
        let key = pool.submit("blocking".to_string(), make_meta(1)).unwrap();

        // Retrieve using blocking API
        let result = pool.retrieve(&key, Duration::from_secs(5)).unwrap();
        assert_eq!(result, "Result: blocking");
    }
}
