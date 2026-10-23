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
//! - **No polling, no parked waiters**: workers block on a channel of
//!   admitted tasks; results go to per-task slots (`super::slots`, cr-22)
//!   that async callers await without holding a thread. Unretrieved results
//!   are evicted after the result TTL (default: the pool's task timeout).
//! - **Clean shutdown**: dropping the sender unblocks workers naturally;
//!   parked tasks are failed with [`TaskError::Cancelled`] rather than dropped
//!   silently.
//! - **Panic containment** (cr-21): an executor panic fails its task with
//!   [`TaskError::ExecutorPanicked`], returns its units through the
//!   dispatcher and leaves the worker serving. A panic that gets past that
//!   (see `crate::contain`) is caught per task on the worker: the task is
//!   reported as panicked if it had no outcome yet, the units its permit
//!   freed while unwinding are handed to parked work with `Dispatcher::poke`,
//!   and the worker thread is replaced by a fresh one, so the pool keeps its
//!   worker count.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use parking_lot::{Mutex, RwLock};
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

use super::slots::{Missing, ResultSlots, SlotId};
use crate::contain::{Panicked, catch, contain};

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

/// The error for a result evicted unread after its TTL.
fn result_expired(key: &MailboxKey) -> SchedulerError {
    SchedulerError::ResultExpired {
        key: Some(mailbox_key_to_string(key)),
    }
}

/// Map a wait that produced no value to the caller's error.
fn missing_error(key: &MailboxKey, missing: Missing) -> SchedulerError {
    match missing {
        Missing::NotFound => result_not_found(key),
        Missing::Expired => result_expired(key),
        Missing::TimedOut => wait_timed_out(),
    }
}

/// The slot id of a key issued by this pool (`generate_mailbox_key`), or
/// `None` for any other key.
fn slot_id(key: &MailboxKey) -> Option<SlotId> {
    if key.tenant != POOL_NAME || key.user_id.is_some() {
        return None;
    }
    key.session_id.as_deref()?.parse().ok()
}

/// Shortest interval at which an idle worker sweeps expired results, so a
/// tiny TTL cannot turn the idle wait into a spin.
const MIN_SWEEP_INTERVAL: Duration = Duration::from_millis(50);

/// An admitted task on its way to a worker.
type Admitted<P> = Grant<WorkerTask<P>>;

/// State shared by the pool handle and its worker threads.
struct Shared<P, R, E> {
    /// Pool configuration.
    config: WorkerPoolConfig,
    /// Admission, the parked queue and the semaphore.
    dispatcher: Dispatcher<WorkerTask<P>>,
    /// Per-task result slots with TTL eviction.
    results: ResultSlots<TaskOutcome<R>>,
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
    /// Worker thread handles, joined by `shutdown`. A replacement worker's
    /// handle is pushed here too.
    workers: Mutex<Vec<JoinHandle<()>>>,
    /// Worker threads serving the pool. A worker that dies is replaced and
    /// its replacement takes its place, so a death does not change this; it
    /// drops when a worker exits (shutdown) or cannot be replaced.
    live_workers: AtomicUsize,
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

    /// The pool clock's monotonic reading, which result TTLs are measured on.
    fn monotonic(&self) -> Duration {
        self.clock.read().monotonic()
    }

    /// How long an idle worker waits for work before sweeping expired
    /// results: the result TTL, but at least [`MIN_SWEEP_INTERVAL`].
    fn sweep_interval(&self) -> Duration {
        self.results.ttl().max(MIN_SWEEP_INTERVAL)
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
        let stored =
            slot_id(key).is_some_and(|id| self.results.fill(id, outcome, self.monotonic()));
        if !stored {
            error!(key = %mailbox_key_to_string(key), "no result slot for a finished task");
        }
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

    /// Run one admitted task on the calling worker behind the panic backstop.
    ///
    /// Returns `Err` when a panic escaped [`execute`](Self::execute)'s own
    /// containment; the worker must then be replaced. Before returning, the
    /// task is reported as panicked unless its completion had started, and
    /// the units its permit freed while unwinding are handed to parked work.
    fn execute_supervised(
        &self,
        runtime: &tokio::runtime::Runtime,
        admitted: Admitted<P>,
    ) -> Result<(), Panicked> {
        let task_id = admitted.item.meta.id;
        let mailbox_key = admitted.item.mailbox_key.clone();
        let mut reported = false;
        let ran = catch(|| self.execute(runtime, admitted, &mut reported));
        if let Err(panicked) = &ran {
            error!(
                task_id,
                %panicked,
                "panic escaped the executor containment; replacing the worker"
            );
            if !reported {
                self.finish(&mailbox_key, Err(TaskError::from(panicked.clone())));
            }
            let woken = self.dispatcher.poke(self.now_ms());
            self.launch(woken);
        }
        ran
    }

    /// Run one admitted task on the calling worker under its budget, then
    /// return its permit and hand on whatever that admits.
    ///
    /// The executor call, its future and the budget timer run inside one
    /// [`contain`], so an executor panic — in the call, in a poll, or in the
    /// drop of its future when the budget elapses — becomes
    /// [`TaskError::ExecutorPanicked`] and the task finishes normally.
    /// `reported` is set just before the completion starts.
    fn execute(
        &self,
        runtime: &tokio::runtime::Runtime,
        admitted: Admitted<P>,
        reported: &mut bool,
    ) {
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
            let executor = &self.executor;
            let contained = runtime.block_on(contain(async move {
                tokio::time::timeout(budget, executor.execute(payload, meta)).await
            }));
            match contained {
                Ok(Ok(outcome)) => outcome,
                Ok(Err(_elapsed)) => {
                    warn!(
                        task_id,
                        ?budget,
                        "task exceeded its budget and was cancelled"
                    );
                    Err(TaskError::Timeout { budget })
                }
                Err(panicked) => {
                    error!(task_id, %panicked, "executor panicked; task failed");
                    Err(TaskError::from(panicked))
                }
            }
        };

        // Return the units before publishing the outcome, so a caller that
        // has retrieved the result already sees them released.
        let now_ms = self.now_ms();
        *reported = true;
        let woken = self.dispatcher.complete(permit, now_ms);
        self.finish(&mailbox_key, outcome);
        self.launch(woken);
    }

    /// Replace a worker that died: spawn a new thread (with a new runtime)
    /// on the same channel. The replacement takes the dead worker's place in
    /// `live_workers`.
    ///
    /// The shutdown flag is read under the `workers` lock, which `shutdown`
    /// also takes after setting the flag, so a replacement is either joined
    /// by `shutdown` or never spawned. A worker that is not replaced drains
    /// the admitted tasks still in the channel, failing each as cancelled,
    /// in case it was the last worker.
    fn respawn(
        self: &Arc<Self>,
        worker_id: usize,
        ready_rx: &Receiver<Admitted<P>>,
        stack_size: usize,
    ) {
        let mut workers = self.workers.lock();
        // Acquire pairs with the Release in `shutdown()`; the lock orders it
        // after the swap as well.
        let replaced = if self.shutdown.load(Ordering::Acquire) {
            false
        } else {
            match spawn_worker(worker_id, ready_rx.clone(), Arc::clone(self), stack_size) {
                Ok(handle) => {
                    workers.push(handle);
                    info!(worker_id, "replaced a worker that panicked");
                    true
                }
                Err(spawn_error) => {
                    error!(worker_id, error = %spawn_error, "could not replace a dead worker");
                    false
                }
            }
        };
        drop(workers);
        if !replaced {
            // Relaxed: a statistic.
            self.live_workers.fetch_sub(1, Ordering::Relaxed);
            for Grant { item: task, permit } in ready_rx.try_iter() {
                drop(permit);
                self.finish(&task.mailbox_key, Err(shutdown_cancelled()));
            }
        }
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
/// - **No polling**: workers block on channel recv; a result waiter is a
///   suspended future (async) or the caller's own parked thread (blocking),
///   never a blocking-pool thread.
/// - **Result TTL**: a result not retrieved within the result TTL (default:
///   the pool's `default_timeout_ms`) is evicted; see
///   [`with_result_ttl`](Self::with_result_ttl).
/// - **Clean shutdown**: Dropping sender naturally unblocks all workers
pub struct WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// State shared with the workers.
    shared: Arc<Shared<P, R, E>>,

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
            results: ResultSlots::new(config.default_timeout()),
            counters: PoolCounters::default(),
            ready_tx: Mutex::new(Some(ready_tx)),
            clock: RwLock::new(Arc::new(SystemClock::new())),
            shutdown: AtomicBool::new(false),
            executor,
            workers: Mutex::new(Vec::new()),
            live_workers: AtomicUsize::new(config.worker_count),
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
        *shared.workers.lock() = workers;

        info!(
            worker_count = config.worker_count,
            max_units = config.max_units,
            max_queue_depth = config.max_queue_depth,
            "WorkerPool initialized with dedicated OS threads (no-polling design)"
        );

        Ok(Self {
            shared,
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

    /// Keep each result that nobody retrieves for `ttl` after its task
    /// finishes, then evict it (default: the pool's `default_timeout_ms`).
    ///
    /// The TTL is measured on the pool clock's monotonic reading (see
    /// [`with_clock`](Self::with_clock)) and applies to results stored from
    /// now on. A `retrieve` of an evicted result fails once with
    /// [`SchedulerError::ResultExpired`], then with `ResultNotFound`.
    /// Eviction happens on the next pool operation after the TTL, or within
    /// one sweep interval (the TTL, at least 50 ms) on an idle pool.
    #[must_use]
    pub fn with_result_ttl(self, ttl: Duration) -> Self {
        self.shared.results.set_ttl(ttl);
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
        shared.results.create(task_id, shared.monotonic());

        let task = WorkerTask {
            payload,
            meta,
            mailbox_key: mailbox_key.clone(),
        };
        let admission = match shared.dispatcher.submit(task, now_ms) {
            Ok(admission) => admission,
            Err(error) => {
                shared.results.discard(task_id);
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

    /// Retrieve a result asynchronously, waiting up to `timeout`.
    ///
    /// The wait is a suspended future under a tokio timer: it occupies no
    /// thread, so any number of concurrent callers fit on a runtime whose
    /// blocking pool is exhausted. A result is taken by the first caller; if
    /// the wait times out the result stays in its slot, and a later call can
    /// still take it until the result TTL evicts it. Cancel-safe: dropping
    /// the future leaves the result in place.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::Task`] carrying the [`TaskError`] when the executor
    ///   failed, ran past its budget ([`TaskError::Timeout`]), expired while
    ///   queued or was cancelled at shutdown
    /// - [`SchedulerError::Timeout`] if the result is not available within the
    ///   timeout
    /// - [`SchedulerError::ResultExpired`] if the result was evicted unread
    ///   after the result TTL (reported once)
    /// - [`SchedulerError::ResultNotFound`] if the key was not issued by this
    ///   pool, its result was already taken, or its expiry was already
    ///   reported
    pub async fn retrieve_async(
        &self,
        key: &MailboxKey,
        timeout: Duration,
    ) -> Result<R, SchedulerError> {
        let Some(id) = slot_id(key) else {
            return Err(result_not_found(key));
        };
        let shared = &self.shared;
        match shared
            .results
            .wait(id, timeout, || shared.monotonic())
            .await
        {
            Ok(outcome) => outcome.map_err(SchedulerError::Task),
            Err(missing) => Err(missing_error(key, missing)),
        }
    }

    /// Retrieve a result, blocking the calling thread up to `timeout`.
    ///
    /// For synchronous callers: a plain thread or a `spawn_blocking`
    /// closure. From async code use [`retrieve_async`](Self::retrieve_async).
    ///
    /// # Errors
    ///
    /// As [`retrieve_async`](Self::retrieve_async).
    ///
    /// # Panics
    ///
    /// When called on a thread that is driving async tasks (inside a tokio
    /// runtime's `block_on` or an async task), with tokio's "Cannot block the
    /// current thread from within a runtime" message, instead of parking a
    /// runtime thread.
    #[track_caller]
    pub fn retrieve(&self, key: &MailboxKey, timeout: Duration) -> Result<R, SchedulerError> {
        let Some(id) = slot_id(key) else {
            return Err(result_not_found(key));
        };
        let shared = &self.shared;
        match shared
            .results
            .wait_blocking(id, timeout, || shared.monotonic())
        {
            Ok(outcome) => outcome.map_err(SchedulerError::Task),
            Err(missing) => Err(missing_error(key, missing)),
        }
    }

    /// Get current pool statistics.
    ///
    /// `worker_count` is the number of worker threads serving the pool: a
    /// worker that dies from a panic is replaced, so it stays at the
    /// configured count until shutdown. `failed_tasks` counts executor
    /// errors and panics, timeouts, expired deadlines and shutdown
    /// cancellations.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let shared = &self.shared;
        // Relaxed: a statistic.
        let live_workers = shared.live_workers.load(Ordering::Relaxed);
        let mut stats = shared
            .counters
            .snapshot(live_workers, shared.config.max_units);
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

        // Join workers with timeout. A replacement for a worker that died is
        // spawned only while the flag is clear and under this lock, so none
        // can appear after this drain.
        let workers: Vec<_> = shared.workers.lock().drain(..).collect();
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
                    // Relaxed: a statistic.
                    shared.live_workers.fetch_sub(1, Ordering::Relaxed);
                    return;
                }
            };

            // Worker loop - blocking recv, NO POLLING. An idle worker wakes
            // once per sweep interval to evict expired results. When the
            // sender is dropped (shutdown) and the channel is empty, recv
            // returns Disconnected and the worker exits.
            loop {
                let admitted = match ready_rx.recv_timeout(shared.sweep_interval()) {
                    Ok(admitted) => admitted,
                    Err(RecvTimeoutError::Timeout) => {
                        shared.results.sweep(shared.monotonic());
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                if shared.execute_supervised(&runtime, admitted).is_err() {
                    // This thread's runtime unwound mid-task; hand the
                    // channel to a fresh worker and exit.
                    drop(runtime);
                    Shared::respawn(&shared, worker_id, &ready_rx, stack_size);
                    return;
                }
            }

            // Relaxed: a statistic.
            shared.live_workers.fetch_sub(1, Ordering::Relaxed);
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
