//! Native implementation of `WorkerPool` using OS threads.
//!
//! This implementation spawns dedicated OS threads that each have their own
//! single-threaded tokio runtime. This ensures CPU/GPU-bound work does not
//! block the main async runtime.
//!
//! # Design Principles
//!
//! - **No polling**: Uses proper signaling (Condvar for blocking, oneshot for async)
//! - **Lock-free fast path**: Result storage uses RwLock with brief critical sections
//! - **Clean shutdown**: Dropping the sender unblocks workers naturally

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::{Condvar, Mutex, RwLock};
use tracing::{debug, error, info, warn};

use crate::config::WorkerPoolConfig;
use crate::core::executor::WorkerExecutor;
use crate::core::TaskMetadata;
use crate::util::serde::MailboxKey;

use super::{generate_mailbox_key, mailbox_key_to_string, PoolCounters, PoolError, PoolStats, WorkerTask};

/// Result entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultState {
    /// Waiting for result.
    Pending,
    /// Result is ready.
    Ready,
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
/// - RwLock for the entry map (read-heavy, write on create/remove)
/// - Per-entry Mutex + Condvar for waiting (lock only when blocking wait needed)
/// - Lock-free check via state atomic would be ideal but Condvar needs Mutex
struct ResultStorage<R> {
    /// Map from mailbox key to (entry, condvar) pair.
    /// The Condvar is used for blocking wait, paired with entry's mutex.
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
    fn wait_for_result(&self, key: &MailboxKey, timeout: Duration) -> Result<R, PoolError> {
        let key_str = mailbox_key_to_string(key);
        
        // Get the entry pair (need to hold Arc while waiting)
        let entry_pair = {
            let entries = self.entries.read();
            entries.get(&key_str).cloned()
        };
        
        let Some(entry_pair) = entry_pair else {
            return Err(PoolError::ResultNotFound);
        };
        
        let (entry_mutex, condvar) = entry_pair.as_ref();
        let mut entry = entry_mutex.lock();
        
        // Fast path: result already ready
        if entry.state == ResultState::Ready {
            return entry.result.take().ok_or(PoolError::ResultNotFound);
        }
        
        // Wait with timeout using Condvar (NO POLLING)
        let wait_result = condvar.wait_for(&mut entry, timeout);
        
        if wait_result.timed_out() {
            return Err(PoolError::Timeout);
        }
        
        if entry.state == ResultState::Ready {
            entry.result.take().ok_or(PoolError::ResultNotFound)
        } else {
            Err(PoolError::Timeout)
        }
    }
    
    /// Remove a result entry entirely.
    fn remove(&self, key: &MailboxKey) -> Option<R> {
        let key_str = mailbox_key_to_string(key);
        
        let mut entries = self.entries.write();
        if let Some(entry_pair) = entries.remove(&key_str) {
            let (entry_mutex, _) = entry_pair.as_ref();
            let mut entry = entry_mutex.lock();
            entry.result.take()
        } else {
            None
        }
    }
    
    /// Get entry for async waiting (returns clone of Arc).
    fn get_entry(&self, key: &MailboxKey) -> Option<Arc<(Mutex<ResultEntry<R>>, Condvar)>> {
        let key_str = mailbox_key_to_string(key);
        let entries = self.entries.read();
        entries.get(&key_str).cloned()
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
/// - **Lock-free fast path**: Atomic counters, RwLock for read-heavy maps
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
    results: Arc<ResultStorage<R>>,
    
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
    /// Returns `PoolError::InvalidConfig` if the configuration is invalid.
    pub fn new(config: WorkerPoolConfig, executor: E) -> Result<Self, PoolError> {
        config.validate().map_err(PoolError::InvalidConfig)?;
        
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
    /// - `PoolError::QueueFull` if the task queue is full
    /// - `PoolError::PoolShutdown` if the pool has been shut down
    pub async fn submit_async(
        &self,
        payload: P,
        meta: TaskMetadata,
    ) -> Result<MailboxKey, PoolError> {
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
    /// - `PoolError::QueueFull` if the task queue is full
    /// - `PoolError::PoolShutdown` if the pool has been shut down
    pub fn submit(&self, payload: P, meta: TaskMetadata) -> Result<MailboxKey, PoolError> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(PoolError::PoolShutdown);
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
            return Err(PoolError::PoolShutdown);
        };
        
        // Try to enqueue (non-blocking)
        match task_tx.try_send(task) {
            Ok(()) => {
                self.counters.submitted_tasks.fetch_add(1, Ordering::Relaxed);
                self.counters.queued_tasks.fetch_add(1, Ordering::Relaxed);
                debug!(task_id = task_id, "Task submitted to worker pool");
                Ok(mailbox_key)
            }
            Err(crossbeam_channel::TrySendError::Full(_)) => {
                // Remove the result slot we created
                self.results.remove(&mailbox_key);
                warn!("Worker pool queue is full");
                Err(PoolError::QueueFull)
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                self.results.remove(&mailbox_key);
                Err(PoolError::PoolShutdown)
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
    /// - `PoolError::Timeout` if the result is not available within the timeout
    /// - `PoolError::ResultNotFound` if the mailbox key is invalid
    pub async fn retrieve_async(
        &self,
        key: &MailboxKey,
        timeout: Duration,
    ) -> Result<R, PoolError> {
        // First, try immediate retrieval (fast path)
        if let Some(result) = self.results.try_retrieve(key) {
            self.results.remove(key);
            return Ok(result);
        }
        
        // Get entry for waiting
        let entry_pair = self.results.get_entry(key)
            .ok_or(PoolError::ResultNotFound)?;
        
        // Use tokio::task::spawn_blocking to wait on the parking_lot Condvar
        // This moves the blocking wait to tokio's blocking thread pool
        // parking_lot's Condvar is significantly faster than std's
        let key_clone = key.clone();
        
        let result = tokio::time::timeout(timeout, async move {
            // Use spawn_blocking for the Condvar wait
            tokio::task::spawn_blocking(move || {
                let (entry_mutex, condvar) = entry_pair.as_ref();
                let mut entry = entry_mutex.lock();
                
                // Check if already ready (fast path, no wait needed)
                if entry.state == ResultState::Ready {
                    return entry.result.take();
                }
                
                // Wait on parking_lot Condvar (blocking, but in spawn_blocking thread)
                // parking_lot's wait is more efficient than std::sync::Condvar
                condvar.wait(&mut entry);
                
                if entry.state == ResultState::Ready {
                    entry.result.take()
                } else {
                    None
                }
            }).await.ok().flatten()
        }).await;
        
        // Clean up the entry
        self.results.remove(&key_clone);
        
        match result {
            Ok(Some(r)) => Ok(r),
            Ok(None) => Err(PoolError::ResultNotFound),
            Err(_) => Err(PoolError::Timeout),
        }
    }
    
    /// Retrieve a result (blocking API) with timeout.
    ///
    /// This method blocks the current thread until the result is available
    /// or the timeout expires. Uses Condvar for efficient waiting - NO POLLING.
    ///
    /// # Errors
    ///
    /// - `PoolError::Timeout` if the result is not available within the timeout
    /// - `PoolError::ResultNotFound` if the mailbox key is invalid
    pub fn retrieve(&self, key: &MailboxKey, timeout: Duration) -> Result<R, PoolError> {
        let result = self.results.wait_for_result(key, timeout);
        // Clean up entry on any outcome
        self.results.remove(key);
        result
    }
    
    /// Get current pool statistics.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let mut stats = self.counters.snapshot(self.config.worker_count, self.config.max_units);
        stats.used_units = self.active_units.load(Ordering::Relaxed);
        stats
    }
    
    /// Shut down the pool gracefully with timeout.
    ///
    /// This drops the task sender to unblock idle workers, then attempts to join
    /// all workers with a reasonable timeout (2 seconds per worker).
    /// 
    /// Workers that don't exit within the timeout are detached to prevent hangs.
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
            
            // Wait up to 2 seconds for this worker to exit
            match rx.recv_timeout(Duration::from_secs(2)) {
                Ok(true) => {
                    debug!(worker_id = idx, "Worker joined successfully");
                }
                Ok(false) => {
                    warn!(worker_id = idx, "Worker panicked");
                }
                Err(_) => {
                    warn!(worker_id = idx, "Worker did not exit within timeout - detaching");
                    // Detach the join thread - worker will eventually exit
                }
            }
            
            // Clean up join thread
            let _ = join_thread.join();
        }
        
        info!(worker_count = worker_count, "Worker pool shut down complete");
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
            let mut task_tx = self.task_tx.lock();
            *task_tx = None;
            
            // DON'T join workers here - let OS clean up threads
            // Explicit shutdown() is required for graceful cleanup
            debug!("WorkerPool dropped without explicit shutdown - workers will be detached");
        }
    }
}

/// Spawn a worker thread.
fn spawn_worker<P, R, E>(
    worker_id: usize,
    task_rx: Receiver<WorkerTask<P>>,
    results: Arc<ResultStorage<R>>,
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
                let task = match task_rx.recv() {
                    Ok(task) => task,
                    Err(_) => {
                        // Channel closed (sender dropped) - clean exit
                        debug!(worker_id = worker_id, "Worker channel closed, exiting");
                        break;
                    }
                };
                
                // Check shutdown flag (in case of shutdown during task processing)
                if shutdown.load(Ordering::Acquire) {
                    debug!(worker_id = worker_id, "Worker shutdown during task, exiting");
                    break;
                }
                
                // Update counters (lock-free atomics)
                counters.queued_tasks.fetch_sub(1, Ordering::Relaxed);
                counters.active_tasks.fetch_add(1, Ordering::Relaxed);
                active_units.fetch_add(task.meta.cost.units, Ordering::Relaxed);
                
                let task_id = task.meta.id;
                let task_cost = task.meta.cost.units;
                let mailbox_key = task.mailbox_key.clone();
                
                debug!(
                    worker_id = worker_id,
                    task_id = task_id,
                    cost = task_cost,
                    "Worker executing task"
                );
                
                // Execute the task in this worker's runtime
                let result = rt.block_on(async {
                    executor.execute(task.payload, task.meta).await
                });
                
                debug!(
                    worker_id = worker_id,
                    task_id = task_id,
                    "Worker completed task"
                );
                
                // Store result and notify waiters (via Condvar)
                results.store(&mailbox_key, result);
                
                // Update counters (lock-free atomics)
                counters.active_tasks.fetch_sub(1, Ordering::Relaxed);
                active_units.fetch_sub(task_cost, Ordering::Relaxed);
                counters.completed_tasks.fetch_add(1, Ordering::Relaxed);
            }
            
            debug!(worker_id = worker_id, "Worker thread exiting");
        })
        .expect("Failed to spawn worker thread")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::serde::{ResourceCost, ResourceKind};
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    
    /// Test executor that records which thread it runs on.
    #[derive(Clone)]
    struct TestExecutor {
        execution_count: Arc<AtomicUsize>,
    }
    
    #[async_trait]
    impl WorkerExecutor<String, String> for TestExecutor {
        async fn execute(&self, payload: String, _meta: TaskMetadata) -> String {
            self.execution_count.fetch_add(1, Ordering::Relaxed);
            // Simulate some work
            tokio::time::sleep(Duration::from_millis(10)).await;
            format!("Result: {}", payload)
        }
    }
    
    fn make_meta(id: u64) -> TaskMetadata {
        TaskMetadata {
            id,
            mailbox: None,
            priority: crate::util::serde::Priority::Normal,
            cost: ResourceCost {
                kind: ResourceKind::Cpu,
                units: 1,
            },
            deadline_ms: None,
            created_at_ms: 0,
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
        let key = pool.submit_async("hello".to_string(), make_meta(1)).await.unwrap();
        
        // Retrieve result
        let result = pool.retrieve_async(&key, Duration::from_secs(5)).await.unwrap();
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
            let key = pool.submit_async(format!("task-{}", i), make_meta(i)).await.unwrap();
            keys.push(key);
        }
        
        // Retrieve all results
        for (i, key) in keys.iter().enumerate() {
            let result = pool.retrieve_async(key, Duration::from_secs(10)).await.unwrap();
            assert_eq!(result, format!("Result: task-{}", i));
        }
        
        // Check execution count
        assert_eq!(executor.execution_count.load(Ordering::Relaxed), 10);
    }
    
    #[test]
    fn test_worker_pool_blocking_api() {
        let executor = TestExecutor {
            execution_count: Arc::new(AtomicUsize::new(0)),
        };
        
        let config = WorkerPoolConfig::new()
            .with_worker_count(2)
            .with_max_queue_depth(10);
        
        let pool = WorkerPool::new(config, executor.clone()).unwrap();
        
        // Submit using blocking API
        let key = pool.submit("blocking".to_string(), make_meta(1)).unwrap();
        
        // Retrieve using blocking API
        let result = pool.retrieve(&key, Duration::from_secs(5)).unwrap();
        assert_eq!(result, "Result: blocking");
    }
}
