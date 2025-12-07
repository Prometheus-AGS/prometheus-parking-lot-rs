//! WASM implementation of `WorkerPool` using async tasks.
//!
//! This implementation uses tokio async tasks instead of OS threads,
//! providing cooperative multitasking suitable for WASM environments.
//!
//! # Design Principles
//!
//! - **No polling**: Uses oneshot channels for result notification
//! - **Async-native**: All operations are async, no blocking
//! - **Semaphore-based concurrency**: Efficient permit-based limiting

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use tokio::sync::{oneshot, Semaphore};
use tracing::{debug, error, info, warn};

use crate::config::WorkerPoolConfig;
use crate::core::executor::WorkerExecutor;
use crate::core::TaskMetadata;
use crate::util::serde::MailboxKey;

use super::{generate_mailbox_key, mailbox_key_to_string, PoolCounters, PoolError, PoolStats};

/// Result entry state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResultState {
    /// Waiting for result.
    Pending,
    /// Result is ready.
    Ready,
}

/// Result storage entry with oneshot notification.
struct ResultEntry<R> {
    /// The result value (once available).
    result: Option<R>,
    /// State of this entry.
    state: ResultState,
    /// Oneshot sender for async notification.
    notify_tx: Option<oneshot::Sender<()>>,
}

/// Result storage for the worker pool.
struct ResultStorage<R> {
    /// Map from mailbox key to result entry.
    entries: RwLock<HashMap<String, Mutex<ResultEntry<R>>>>,
}

impl<R> ResultStorage<R> {
    fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }
    
    /// Create a slot for a result and return a oneshot receiver for notification.
    fn create_slot(&self, key: &MailboxKey) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        let key_str = mailbox_key_to_string(key);
        
        let entry = ResultEntry {
            result: None,
            state: ResultState::Pending,
            notify_tx: Some(tx),
        };
        
        let mut entries = self.entries.write();
        entries.insert(key_str, Mutex::new(entry));
        
        rx
    }
    
    /// Store a result and notify any waiters.
    fn store(&self, key: &MailboxKey, result: R) {
        let key_str = mailbox_key_to_string(key);
        
        let entries = self.entries.read();
        if let Some(entry_mutex) = entries.get(&key_str) {
            let mut entry = entry_mutex.lock();
            entry.result = Some(result);
            entry.state = ResultState::Ready;
            // Notify waiter if any
            if let Some(tx) = entry.notify_tx.take() {
                let _ = tx.send(());
            }
        }
    }
    
    /// Try to retrieve a result immediately.
    fn try_retrieve(&self, key: &MailboxKey) -> Option<R> {
        let key_str = mailbox_key_to_string(key);
        
        let entries = self.entries.read();
        if let Some(entry_mutex) = entries.get(&key_str) {
            let mut entry = entry_mutex.lock();
            if entry.state == ResultState::Ready {
                return entry.result.take();
            }
        }
        None
    }
    
    /// Remove a result entry entirely.
    fn remove(&self, key: &MailboxKey) -> Option<R> {
        let key_str = mailbox_key_to_string(key);
        
        let mut entries = self.entries.write();
        if let Some(entry_mutex) = entries.remove(&key_str) {
            let mut entry = entry_mutex.lock();
            entry.result.take()
        } else {
            None
        }
    }
    
    /// Get the oneshot receiver for a key (for async waiting).
    fn get_notify_rx(&self, key: &MailboxKey) -> Option<oneshot::Receiver<()>> {
        let key_str = mailbox_key_to_string(key);
        
        let entries = self.entries.read();
        if let Some(entry_mutex) = entries.get(&key_str) {
            let mut entry = entry_mutex.lock();
            // Create new receiver if needed
            if entry.notify_tx.is_none() && entry.state == ResultState::Pending {
                let (tx, rx) = oneshot::channel();
                entry.notify_tx = Some(tx);
                return Some(rx);
            }
        }
        None
    }
}

/// Worker pool using async tasks for WASM environments.
///
/// This implementation uses tokio async tasks with a semaphore for concurrency
/// control. Unlike the native implementation, there are no blocking APIs since
/// WASM cannot block.
pub struct WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// Pool configuration.
    config: WorkerPoolConfig,
    
    /// Executor for task execution.
    executor: E,
    
    /// Semaphore for concurrency control.
    semaphore: Arc<Semaphore>,
    
    /// Result storage with notification support.
    results: Arc<ResultStorage<R>>,
    
    /// Pool statistics counters (lock-free).
    counters: Arc<PoolCounters>,
    
    /// Active resource units (lock-free).
    active_units: Arc<AtomicU32>,
    
    /// Shutdown flag (lock-free).
    shutdown: Arc<AtomicBool>,
    
    /// Task ID counter (lock-free).
    task_id_counter: AtomicU64,
    
    /// Phantom data for payload type.
    _payload: std::marker::PhantomData<P>,
}

impl<P, R, E> WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// Create a new worker pool with the given configuration and executor.
    ///
    /// On WASM, this creates a pool of async tasks limited by a semaphore.
    ///
    /// # Errors
    ///
    /// Returns `PoolError::InvalidConfig` if the configuration is invalid.
    pub fn new(config: WorkerPoolConfig, executor: E) -> Result<Self, PoolError> {
        config.validate().map_err(PoolError::InvalidConfig)?;
        
        let semaphore = Arc::new(Semaphore::new(config.worker_count));
        let results = Arc::new(ResultStorage::new());
        let counters = Arc::new(PoolCounters::default());
        let active_units = Arc::new(AtomicU32::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));
        
        info!(
            worker_count = config.worker_count,
            max_units = config.max_units,
            max_queue_depth = config.max_queue_depth,
            "WorkerPool (WASM) initialized with async tasks"
        );
        
        Ok(Self {
            config,
            executor,
            semaphore,
            results,
            counters,
            active_units,
            shutdown,
            task_id_counter: AtomicU64::new(0),
            _payload: std::marker::PhantomData,
        })
    }
    
    /// Submit a task asynchronously.
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
        if self.shutdown.load(Ordering::Acquire) {
            return Err(PoolError::PoolShutdown);
        }
        
        // Check queue depth
        let current_queued = self.counters.queued_tasks.load(Ordering::Relaxed);
        if current_queued >= self.config.max_queue_depth as u64 {
            warn!("Worker pool queue is full");
            return Err(PoolError::QueueFull);
        }
        
        // Generate unique task ID and mailbox key
        let task_id = self.task_id_counter.fetch_add(1, Ordering::Relaxed);
        let mailbox_key = generate_mailbox_key(task_id);
        
        // Create result slot with notification
        let _notify_rx = self.results.create_slot(&mailbox_key);
        
        // Update counters
        self.counters.submitted_tasks.fetch_add(1, Ordering::Relaxed);
        self.counters.queued_tasks.fetch_add(1, Ordering::Relaxed);
        
        // Clone refs for the spawned task
        let semaphore = Arc::clone(&self.semaphore);
        let results = Arc::clone(&self.results);
        let counters = Arc::clone(&self.counters);
        let active_units = Arc::clone(&self.active_units);
        let shutdown = Arc::clone(&self.shutdown);
        let executor = self.executor.clone();
        let task_cost = meta.cost.units;
        let key_clone = mailbox_key.clone();
        
        // Spawn async task
        tokio::spawn(async move {
            // Acquire semaphore permit (efficient async wait, no polling)
            let _permit = match semaphore.acquire().await {
                Ok(permit) => permit,
                Err(_) => {
                    // Semaphore closed
                    counters.queued_tasks.fetch_sub(1, Ordering::Relaxed);
                    return;
                }
            };
            
            // Check shutdown
            if shutdown.load(Ordering::Acquire) {
                counters.queued_tasks.fetch_sub(1, Ordering::Relaxed);
                return;
            }
            
            // Update counters
            counters.queued_tasks.fetch_sub(1, Ordering::Relaxed);
            counters.active_tasks.fetch_add(1, Ordering::Relaxed);
            active_units.fetch_add(task_cost, Ordering::Relaxed);
            
            debug!(task_id = task_id, "WASM worker executing task");
            
            // Execute the task
            let result = executor.execute(payload, meta).await;
            
            debug!(task_id = task_id, "WASM worker completed task");
            
            // Store result and notify waiters
            results.store(&key_clone, result);
            
            // Update counters
            counters.active_tasks.fetch_sub(1, Ordering::Relaxed);
            active_units.fetch_sub(task_cost, Ordering::Relaxed);
            counters.completed_tasks.fetch_add(1, Ordering::Relaxed);
        });
        
        debug!(task_id = task_id, "Task submitted to WASM worker pool");
        Ok(mailbox_key)
    }
    
    /// Retrieve a result asynchronously with timeout.
    ///
    /// This method waits for the result to become available or times out.
    /// Uses oneshot channel for efficient notification - NO POLLING.
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
        
        // Get notification receiver
        let notify_rx = self.results.get_notify_rx(key);
        
        let Some(notify_rx) = notify_rx else {
            // No entry or already ready - try again
            if let Some(result) = self.results.try_retrieve(key) {
                self.results.remove(key);
                return Ok(result);
            }
            return Err(PoolError::ResultNotFound);
        };
        
        // Wait for notification with timeout (NO POLLING)
        match tokio::time::timeout(timeout, notify_rx).await {
            Ok(Ok(())) => {
                // Notified - result should be available
                let result = self.results.remove(key).ok_or(PoolError::ResultNotFound)?;
                Ok(result)
            }
            Ok(Err(_)) => {
                // Channel closed without result
                self.results.remove(key);
                Err(PoolError::Internal("result notification channel closed".into()))
            }
            Err(_) => {
                // Timeout
                self.results.remove(key);
                Err(PoolError::Timeout)
            }
        }
    }
    
    /// Get current pool statistics.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let mut stats = self.counters.snapshot(self.config.worker_count, self.config.max_units);
        stats.used_units = self.active_units.load(Ordering::Relaxed);
        stats
    }
    
    /// Shut down the pool.
    ///
    /// This signals all workers to stop. Active tasks will complete,
    /// but new submissions will be rejected.
    pub fn shutdown(&self) {
        if self.shutdown.swap(true, Ordering::AcqRel) {
            return; // Already shut down
        }
        
        info!("Shutting down WASM worker pool");
        // Close semaphore to prevent new permits
        self.semaphore.close();
        info!("WASM worker pool shut down signaled");
    }
}

impl<P, R, E> Drop for WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::serde::{ResourceCost, ResourceKind};
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    
    /// Test executor.
    #[derive(Clone)]
    struct TestExecutor {
        execution_count: Arc<AtomicUsize>,
    }
    
    #[async_trait]
    impl WorkerExecutor<String, String> for TestExecutor {
        async fn execute(&self, payload: String, _meta: TaskMetadata) -> String {
            self.execution_count.fetch_add(1, Ordering::Relaxed);
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
    async fn test_wasm_worker_pool_basic() {
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
    }
    
    #[tokio::test]
    async fn test_wasm_worker_pool_multiple_tasks() {
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
}
