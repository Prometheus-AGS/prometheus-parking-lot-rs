//! Native implementation of `WorkerPool` using OS threads.
//!
//! This implementation spawns dedicated OS threads that each have their own
//! single-threaded tokio runtime. This ensures CPU/GPU-bound work does not
//! block the main async runtime.
//!
//! # Design Principles
//!
//! - **No polling**: Uses proper signaling (Condvar for blocking, oneshot for async)
//! - **Lock-free fast path**: Result storage uses `RwLock` with brief critical sections
//! - **Clean shutdown**: Dropping the sender unblocks workers naturally

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, bounded};
use parking_lot::{Condvar, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::dispatch::{
    PoolCounters, WorkerTask, generate_mailbox_key, mailbox_key_to_string,
};
use prometheus_parking_lot_core::{
    MailboxKey, PoolStats, SchedulerError, TaskError, TaskMetadata, WorkerExecutor,
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

/// Worker pool with dedicated OS threads for CPU/GPU-bound work.
///
/// Each worker thread has its own single-threaded tokio runtime, ensuring
/// that executor work does not block the main async runtime.
///
/// # Design
///
/// - **No polling**: Workers block on channel recv; results use Condvar
/// - **Clean shutdown**: Dropping sender naturally unblocks all workers
/// - **Lock-free fast path**: Atomic counters, `RwLock` for read-heavy maps
pub struct WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// Pool configuration.
    config: WorkerPoolConfig,

    /// Task sender (to workers). Option allows clean shutdown by dropping.
    task_tx: Mutex<Option<Sender<WorkerTask<P>>>>,

    /// Result storage with Condvar-based notification.
    results: Arc<ResultStorage<TaskOutcome<R>>>,

    /// Pool statistics counters (lock-free atomics).
    counters: Arc<PoolCounters>,

    /// Active resource units (lock-free atomic).
    active_units: Arc<AtomicU32>,

    /// Shutdown flag (lock-free atomic).
    shutdown: Arc<AtomicBool>,

    /// Worker thread handles.
    workers: Mutex<Vec<JoinHandle<()>>>,

    /// Task ID counter (lock-free atomic).
    task_id_counter: AtomicU64,

    /// Phantom data for executor type.
    _executor: std::marker::PhantomData<E>,
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
        reason = "public constructor takes the config by value; changing it is an API change for the worker-pool rework (cr-19)"
    )]
    pub fn new(config: WorkerPoolConfig, executor: E) -> Result<Self, SchedulerError> {
        config.validate().map_err(SchedulerError::invalid_config)?;

        let (task_tx, task_rx) = bounded::<WorkerTask<P>>(config.max_queue_depth);
        let results = Arc::new(ResultStorage::new());
        let counters = Arc::new(PoolCounters::default());
        let active_units = Arc::new(AtomicU32::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Spawn worker threads
        let mut workers = Vec::with_capacity(config.worker_count);

        for worker_id in 0..config.worker_count {
            let worker = spawn_worker(
                worker_id,
                task_rx.clone(),
                Arc::clone(&results),
                Arc::clone(&counters),
                Arc::clone(&active_units),
                Arc::clone(&shutdown),
                executor.clone(),
                config.thread_stack_size,
            );
            workers.push(worker);
        }

        info!(
            worker_count = config.worker_count,
            max_units = config.max_units,
            max_queue_depth = config.max_queue_depth,
            "WorkerPool initialized with dedicated OS threads (no-polling design)"
        );

        Ok(Self {
            config,
            task_tx: Mutex::new(Some(task_tx)),
            results,
            counters,
            active_units,
            shutdown,
            workers: Mutex::new(workers),
            task_id_counter: AtomicU64::new(0),
            _executor: std::marker::PhantomData,
        })
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
    /// - [`SchedulerError::QueueFull`] if the task queue is full
    /// - [`SchedulerError::PoolShutdown`] if the pool has been shut down
    #[expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "public async API kept for callers; enqueue is non-blocking today and cr-19 may make it await backpressure (the trait-impl variant misfires on an inherent method)"
    )]
    pub async fn submit_async(
        &self,
        payload: P,
        meta: TaskMetadata,
    ) -> Result<MailboxKey, SchedulerError> {
        // Use the sync submit internally - it's non-blocking for enqueue
        self.submit(payload, meta)
    }

    /// Submit a task (blocking API).
    ///
    /// This method can be called from any context. The enqueue operation
    /// itself is non-blocking; it only fails immediately if the queue is full.
    ///
    /// # Returns
    ///
    /// Returns a `MailboxKey` that can be used to retrieve the result.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::QueueFull`] if the task queue is full
    /// - [`SchedulerError::PoolShutdown`] if the pool has been shut down
    #[expect(
        clippy::significant_drop_tightening,
        reason = "narrowing lock scopes changes concurrent behaviour; the worker-pool rework (cr-19/cr-29) owns it"
    )]
    pub fn submit(&self, payload: P, meta: TaskMetadata) -> Result<MailboxKey, SchedulerError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(pool_shutdown());
        }

        // Generate unique task ID and mailbox key
        let task_id = self.task_id_counter.fetch_add(1, Ordering::Relaxed);
        let mailbox_key = generate_mailbox_key(task_id);

        // Create result slot
        self.results.create_slot(&mailbox_key);

        // Create the worker task
        let task = WorkerTask {
            payload,
            meta,
            mailbox_key: mailbox_key.clone(),
        };

        // Get sender (brief lock)
        let task_tx_guard = self.task_tx.lock();
        let Some(task_tx) = task_tx_guard.as_ref() else {
            // Pool is shutting down
            self.results.remove(&mailbox_key);
            return Err(pool_shutdown());
        };

        // Try to enqueue (non-blocking)
        match task_tx.try_send(task) {
            Ok(()) => {
                self.counters
                    .submitted_tasks
                    .fetch_add(1, Ordering::Relaxed);
                self.counters.queued_tasks.fetch_add(1, Ordering::Relaxed);
                debug!(task_id = task_id, "Task submitted to worker pool");
                Ok(mailbox_key)
            }
            Err(crossbeam_channel::TrySendError::Full(_)) => {
                // Remove the result slot we created
                self.results.remove(&mailbox_key);
                warn!("Worker pool queue is full");
                // The bounded channel is full, so the depth is its capacity.
                Err(SchedulerError::queue_full(
                    POOL_NAME,
                    self.config.max_queue_depth,
                    self.config.max_queue_depth,
                ))
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                self.results.remove(&mailbox_key);
                Err(pool_shutdown())
            }
        }
    }

    /// Retrieve a result asynchronously with timeout.
    ///
    /// This method waits for the result to become available or times out.
    /// Uses tokio's async timing - no polling.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::Task`] carrying the [`TaskError`] when the executor
    ///   failed
    /// - [`SchedulerError::Timeout`] if the result is not available within the
    ///   timeout
    /// - [`SchedulerError::ResultNotFound`] if the mailbox key is invalid
    pub async fn retrieve_async(
        &self,
        key: &MailboxKey,
        timeout: Duration,
    ) -> Result<R, SchedulerError> {
        // First, try immediate retrieval (fast path)
        if let Some(outcome) = self.results.try_retrieve(key) {
            self.results.remove(key);
            return outcome.map_err(SchedulerError::Task);
        }

        // Get entry for waiting
        let entry_pair = self
            .results
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
        self.results.remove(key);

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
    /// - [`SchedulerError::Task`] carrying the [`TaskError`] when the executor
    ///   failed
    /// - [`SchedulerError::Timeout`] if the result is not available within the
    ///   timeout
    /// - [`SchedulerError::ResultNotFound`] if the mailbox key is invalid
    pub fn retrieve(&self, key: &MailboxKey, timeout: Duration) -> Result<R, SchedulerError> {
        let outcome = self.results.wait_for_result(key, timeout);
        // Clean up entry on any outcome
        self.results.remove(key);
        outcome?.map_err(SchedulerError::Task)
    }

    /// Get current pool statistics.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let mut stats = self
            .counters
            .snapshot(self.config.worker_count, self.config.max_units);
        stats.used_units = self.active_units.load(Ordering::Relaxed);
        stats
    }

    /// Shut down the pool gracefully with timeout.
    ///
    /// This drops the task sender to unblock idle workers, then attempts to join
    /// all workers with a reasonable timeout (2 seconds per worker).
    ///
    /// Workers that don't exit within the timeout are detached to prevent hangs.
    #[expect(
        clippy::significant_drop_tightening,
        reason = "narrowing lock scopes changes concurrent behaviour; the worker-pool rework (cr-19/cr-29) owns it"
    )]
    pub fn shutdown(&self) {
        // Check if already shut down
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return; // Already shut down
        }

        info!("Shutting down worker pool");

        // Drop the sender to unblock all workers waiting on recv()
        {
            let mut task_tx = self.task_tx.lock();
            *task_tx = None;
        }

        // Join workers with timeout
        let mut workers = self.workers.lock();
        let worker_count = workers.len();

        for (idx, worker) in workers.drain(..).enumerate() {
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
        if !self.shutdown.swap(true, Ordering::AcqRel) {
            // Drop the sender to unblock waiting workers
            *self.task_tx.lock() = None;

            // DON'T join workers here - let OS clean up threads
            // Explicit shutdown() is required for graceful cleanup
            debug!("WorkerPool dropped without explicit shutdown - workers will be detached");
        }
    }
}

/// Spawn a worker thread.
#[expect(
    clippy::too_many_arguments,
    reason = "private thread entry point; parameter grouping belongs to the worker-pool rework (cr-19/cr-29)"
)]
fn spawn_worker<P, R, E>(
    worker_id: usize,
    task_rx: Receiver<WorkerTask<P>>,
    results: Arc<ResultStorage<TaskOutcome<R>>>,
    counters: Arc<PoolCounters>,
    active_units: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
    executor: E,
    stack_size: usize,
) -> JoinHandle<()>
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
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    error!(
                        worker_id = worker_id,
                        error = %e,
                        "Failed to create worker runtime"
                    );
                    return;
                }
            };

            // Worker loop - blocking recv, NO POLLING
            // When sender is dropped, recv() returns Err and worker exits
            loop {
                // Block waiting for a task
                // This is efficient - thread sleeps until work arrives
                // When sender is dropped (shutdown), recv returns Err
                let Ok(task) = task_rx.recv() else {
                    // Channel closed (sender dropped) - clean exit
                    debug!(worker_id = worker_id, "Worker channel closed, exiting");
                    break;
                };

                // Check shutdown flag (in case of shutdown during task processing)
                if shutdown.load(Ordering::Acquire) {
                    debug!(
                        worker_id = worker_id,
                        "Worker shutdown during task, exiting"
                    );
                    break;
                }

                // Update counters (lock-free atomics)
                counters.queued_tasks.fetch_sub(1, Ordering::Relaxed);
                counters.active_tasks.fetch_add(1, Ordering::Relaxed);
                let task_id = task.meta.id;
                let task_cost = task.meta.cost.total_units();
                active_units.fetch_add(task_cost, Ordering::Relaxed);
                let mailbox_key = task.mailbox_key.clone();

                debug!(
                    worker_id = worker_id,
                    task_id = task_id,
                    cost = task_cost,
                    "Worker executing task"
                );

                // Execute the task in this worker's runtime
                let result = rt.block_on(async { executor.execute(task.payload, task.meta).await });

                debug!(
                    worker_id = worker_id,
                    task_id = task_id,
                    "Worker completed task"
                );

                let finished = if result.is_ok() {
                    &counters.completed_tasks
                } else {
                    &counters.failed_tasks
                };

                // Store result and notify waiters (via Condvar)
                results.store(&mailbox_key, result);

                // Update counters (lock-free atomics)
                counters.active_tasks.fetch_sub(1, Ordering::Relaxed);
                active_units.fetch_sub(task_cost, Ordering::Relaxed);
                finished.fetch_add(1, Ordering::Relaxed);
            }

            debug!(worker_id = worker_id, "Worker thread exiting");
        })
        .expect("Failed to spawn worker thread")
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
