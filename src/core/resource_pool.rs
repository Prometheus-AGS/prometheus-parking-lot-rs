//! Resource pool skeleton and core scheduling traits.

use std::future::Future;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use crate::core::{AuditSink, SchedulerError, TaskExecutor, TaskPayload};
use crate::util::serde::{MailboxKey, Priority, ResourceCost, TaskId};

/// Status of a task in the scheduler lifecycle.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TaskStatus {
    /// Task is queued waiting for capacity.
    Queued,
    /// Task is running.
    Running,
    /// Task finished successfully.
    Completed,
    /// Task failed with a reason.
    Failed(String),
    /// Task expired before it could start.
    Expired,
    /// Task was rejected or dropped.
    Dropped(String),
}

/// Metadata describing a scheduled task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskMetadata {
    /// Unique task identifier.
    pub id: TaskId,
    /// Tenant/user/session identification for isolation and mailbox routing.
    pub mailbox: Option<MailboxKey>,
    /// Priority used for queue ordering.
    pub priority: Priority,
    /// Resource cost for capacity accounting.
    pub cost: ResourceCost,
    /// Absolute deadline in milliseconds since epoch.
    pub deadline_ms: Option<u128>,
    /// Creation timestamp in milliseconds since epoch.
    pub created_at_ms: u128,
}

/// A schedulable task with metadata and payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "P: serde::Serialize"))]
#[serde(bound(deserialize = "P: serde::de::DeserializeOwned"))]
pub struct ScheduledTask<P> {
    /// Metadata driving scheduling decisions.
    pub meta: TaskMetadata,
    /// Task payload supplied by caller.
    pub payload: P,
}

/// Abstraction for queue backends.
pub trait TaskQueue<P> {
    /// Enqueue a task if space permits.
    fn enqueue(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError>;
    /// Dequeue the next ready task, honoring priority and deadlines.
    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError>;
    /// Remove expired tasks and return count.
    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError>;
    /// Maximum depth allowed for this queue.
    fn max_depth(&self) -> usize;
    /// Current depth.
    fn len(&self) -> usize;
}

/// Abstraction for mailbox backends.
pub trait Mailbox<T> {
    /// Deliver a task outcome to the mailbox.
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<T>,
    ) -> Result<(), SchedulerError>;
}

/// Abstraction for spawning task execution on a runtime.
pub trait Spawn {
    /// Spawn an async task that returns a future.
    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static;
}

/// Configuration values for capacity enforcement.
#[derive(Debug, Clone)]
pub struct PoolLimits {
    /// Maximum concurrent resource units.
    pub max_units: u32,
    /// Maximum queued tasks.
    pub max_queue_depth: usize,
    /// Default timeout for tasks (seconds).
    pub default_timeout: Duration,
}

/// Shared state for Condvar-based wake notifications.
/// This allows efficient signaling when capacity becomes available.
pub struct WakeState {
    /// Flag indicating capacity may be available.
    pub capacity_available: bool,
    /// Flag to signal shutdown of wake worker.
    pub shutdown: bool,
}

/// Resource pool with capacity accounting and complete parking lot algorithm.
///
/// Uses lock-free `AtomicU32` for capacity tracking (`active_units`),
/// separate `parking_lot::Mutex` for queue and mailbox operations,
/// and `parking_lot::Condvar` for efficient wake notifications.
pub struct ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    limits: PoolLimits,
    /// Lock-free capacity tracking - number of active resource units in use.
    active_units: Arc<AtomicU32>,
    /// Task queue protected by its own mutex for write-heavy operations.
    queue: Arc<Mutex<Q>>,
    /// Mailbox protected by its own mutex, separate from queue for better concurrency.
    mailbox: Arc<Mutex<M>>,
    /// Condition variable for efficient wake notifications.
    /// Signaled when capacity is released to wake waiting workers.
    wake_condvar: Arc<Condvar>,
    /// State protected by mutex for the condvar.
    wake_state: Arc<Mutex<WakeState>>,
    /// Flag indicating if async wake is enabled (vs sync wake worker).
    async_wake_enabled: Arc<AtomicBool>,
    executor: E,
    spawner: S,
    audit: Option<Arc<Mutex<Box<dyn AuditSink>>>>,
    _payload_marker: PhantomData<P>,
    _result_marker: PhantomData<T>,
}

impl<P, T, Q, M, E, S> ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    /// Create a new pool from components.
    pub fn new(limits: PoolLimits, queue: Q, mailbox: M, executor: E, spawner: S) -> Self {
        Self {
            limits,
            active_units: Arc::new(AtomicU32::new(0)),
            queue: Arc::new(Mutex::new(queue)),
            mailbox: Arc::new(Mutex::new(mailbox)),
            wake_condvar: Arc::new(Condvar::new()),
            wake_state: Arc::new(Mutex::new(WakeState {
                capacity_available: false,
                shutdown: false,
            })),
            async_wake_enabled: Arc::new(AtomicBool::new(true)),
            executor,
            spawner,
            audit: None,
            _payload_marker: PhantomData,
            _result_marker: PhantomData,
        }
    }

    /// Attach an audit sink.
    pub fn with_audit(mut self, audit: Box<dyn AuditSink>) -> Self {
        self.audit = Some(Arc::new(Mutex::new(audit)));
        self
    }

    /// Try to reserve capacity atomically using CAS loop.
    /// Returns true if capacity was successfully reserved, false otherwise.
    fn try_reserve_capacity(&self, cost: u32) -> bool {
        let mut current = self.active_units.load(Ordering::Acquire);
        loop {
            if current + cost > self.limits.max_units {
                return false;
            }
            match self.active_units.compare_exchange_weak(
                current,
                current + cost,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }

    /// Check if task can start without acquiring any locks (lock-free read).
    fn can_start_lockfree(&self, cost: u32) -> bool {
        let current = self.active_units.load(Ordering::Acquire);
        current + cost <= self.limits.max_units
    }

    /// Signal shutdown to any waiting wake workers.
    pub fn shutdown(&self) {
        let mut state = self.wake_state.lock();
        state.shutdown = true;
        drop(state);
        // Wake all waiting threads so they can exit
        self.wake_condvar.notify_all();
    }
}

impl<P, T, Q, M, E, S> ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    Q: TaskQueue<P> + Send + 'static,
    M: Mailbox<T> + Send + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Clone + Send + 'static,
{
    /// Submit a task, enforcing capacity, deadlines, and queue depth.
    /// Executes immediately if capacity available, otherwise enqueues.
    pub async fn submit(
        &self,
        task: ScheduledTask<P>,
        now_ms: u128,
    ) -> Result<TaskStatus, SchedulerError> {
        // Check deadline before any processing
        if let Some(deadline) = task.meta.deadline_ms {
            if now_ms > deadline {
                tracing::warn!("task {} expired before enqueue", task.meta.id);
                return Err(SchedulerError::DeadlineExpired);
            }
        }

        // Lock-free capacity check and reservation using CAS
        if self.can_start_lockfree(task.meta.cost.units)
            && self.try_reserve_capacity(task.meta.cost.units)
        {
            // Record audit (sync operation with parking_lot mutex)
            self.record_audit(&task, "start");
            tracing::info!("task {} started immediately", task.meta.id);

            // Spawn execution
            self.spawn_task(task).await;

            return Ok(TaskStatus::Running);
        }

        // Not enough capacity - try to enqueue
        // Quick mutex for queue check and enqueue (parking_lot is fast here)
        {
            let queue = self.queue.lock();
            if queue.len() >= self.limits.max_queue_depth {
                tracing::warn!(
                    "task {} rejected: queue full (depth={})",
                    task.meta.id,
                    queue.len()
                );
                return Err(SchedulerError::QueueFull("max queue depth reached".into()));
            }
        } // Lock released before audit

        // Record audit
        self.record_audit(&task, "enqueue");

        // Enqueue the task
        {
            let mut queue = self.queue.lock();
            queue.enqueue(task)?;
        }
        tracing::info!("task enqueued");
        Ok(TaskStatus::Queued)
    }

    /// Spawn a task execution asynchronously.
    async fn spawn_task(&self, task: ScheduledTask<P>) {
        let executor = self.executor.clone();
        let queue = Arc::clone(&self.queue);
        let mailbox = Arc::clone(&self.mailbox);
        let active_units = Arc::clone(&self.active_units);
        let wake_condvar = Arc::clone(&self.wake_condvar);
        let wake_state = Arc::clone(&self.wake_state);
        let async_wake_enabled = Arc::clone(&self.async_wake_enabled);
        let limits = self.limits.clone();
        let audit = self.audit.clone();
        let spawner = self.spawner.clone();
        let task_id = task.meta.id;
        let task_cost = task.meta.cost.units;
        let mailbox_key = task.meta.mailbox.clone();
        let meta = task.meta.clone();
        let payload = task.payload;

        self.spawner.spawn(async move {
            tracing::debug!("executing task {}", task_id);

            // Execute the task
            let result = executor.execute(payload, meta).await;

            tracing::info!("task {} completed", task_id);

            // Handle task completion
            Self::on_task_finished_static(
                queue,
                mailbox,
                active_units,
                wake_condvar,
                wake_state,
                async_wake_enabled,
                limits,
                audit,
                spawner,
                executor,
                task_id,
                task_cost,
                mailbox_key,
                result,
            )
            .await;
        });
    }

    /// Static helper for task completion handling (callable from spawned task).
    #[allow(clippy::too_many_arguments)]
    fn on_task_finished_static(
        queue: Arc<Mutex<Q>>,
        mailbox: Arc<Mutex<M>>,
        active_units: Arc<AtomicU32>,
        wake_condvar: Arc<Condvar>,
        wake_state: Arc<Mutex<WakeState>>,
        async_wake_enabled: Arc<AtomicBool>,
        limits: PoolLimits,
        audit: Option<Arc<Mutex<Box<dyn AuditSink>>>>,
        spawner: S,
        executor: E,
        task_id: TaskId,
        task_cost: u32,
        mailbox_key: Option<MailboxKey>,
        result: T,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        Box::pin(async move {
            // Release capacity atomically (lock-free)
            active_units.fetch_sub(task_cost, Ordering::Release);
            tracing::debug!(
                "released {} units, active: {}",
                task_cost,
                active_units.load(Ordering::Acquire)
            );

            // Signal capacity available via Condvar (fast, non-blocking)
            {
                let mut state = wake_state.lock();
                state.capacity_available = true;
            }
            wake_condvar.notify_one();

            // Deliver to mailbox if key present (separate mutex from queue)
            if let Some(ref key) = mailbox_key {
                let mut mailbox_guard = mailbox.lock();
                if let Err(e) =
                    mailbox_guard.deliver(key, TaskStatus::Completed, Some(result))
                {
                    tracing::error!("failed to deliver to mailbox: {}", e);
                }
            }

            // Record audit (sync mutex)
            if let Some(audit_sink) = audit.as_ref() {
                let mut sink = audit_sink.lock();
                let tenant = mailbox_key
                    .as_ref()
                    .map(|m| m.tenant.clone())
                    .unwrap_or_else(|| "unknown".into());
                sink.record(crate::core::build_audit_event(
                    format!("{}-complete-{}", task_id, crate::util::clock::now_ms()),
                    task_id.to_string(),
                    "pool",
                    tenant,
                    "complete".to_string(),
                    None,
                ));
            }

            // Try to wake next task using async spawned task (default mode)
            if async_wake_enabled.load(Ordering::Acquire) {
                let spawner_clone = spawner.clone();
                spawner.spawn(Self::try_wake_next_static(
                    queue,
                    mailbox,
                    active_units,
                    wake_condvar,
                    wake_state,
                    async_wake_enabled,
                    limits,
                    audit,
                    spawner_clone,
                    executor,
                ));
            }
            // If async_wake_enabled is false, a dedicated sync wake worker
            // is expected to be waiting on the condvar
        })
    }

    /// Try to wake and start the next queued task if capacity available.
    #[allow(clippy::too_many_arguments)]
    fn try_wake_next_static(
        queue: Arc<Mutex<Q>>,
        mailbox: Arc<Mutex<M>>,
        active_units: Arc<AtomicU32>,
        wake_condvar: Arc<Condvar>,
        wake_state: Arc<Mutex<WakeState>>,
        async_wake_enabled: Arc<AtomicBool>,
        limits: PoolLimits,
        audit: Option<Arc<Mutex<Box<dyn AuditSink>>>>,
        spawner: S,
        executor: E,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        Box::pin(async move {
            loop {
                // Try to dequeue a task (quick sync mutex on queue only)
                let task_opt = {
                    let mut queue_guard = queue.lock();
                    match queue_guard.dequeue() {
                        Ok(task) => task,
                        Err(e) => {
                            tracing::error!("failed to dequeue: {}", e);
                            break;
                        }
                    }
                };

                let task = match task_opt {
                    Some(t) => t,
                    None => {
                        tracing::debug!("queue empty, no tasks to wake");
                        break;
                    }
                };

                // Check if we can start this task (lock-free)
                let current = active_units.load(Ordering::Acquire);
                let can_start = current + task.meta.cost.units <= limits.max_units;

                if !can_start {
                    // Re-enqueue the task and stop (quick sync mutex on queue only)
                    let mut queue_guard = queue.lock();
                    if let Err(e) = queue_guard.enqueue(task) {
                        tracing::error!("failed to re-enqueue task: {}", e);
                    }
                    tracing::debug!("insufficient capacity to wake next task");
                    break;
                }

                // Try to reserve capacity atomically using CAS
                let mut current = active_units.load(Ordering::Acquire);
                let reserved = loop {
                    if current + task.meta.cost.units > limits.max_units {
                        break false;
                    }
                    match active_units.compare_exchange_weak(
                        current,
                        current + task.meta.cost.units,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break true,
                        Err(actual) => current = actual,
                    }
                };

                if !reserved {
                    // Failed to reserve, re-enqueue and stop
                    let mut queue_guard = queue.lock();
                    if let Err(e) = queue_guard.enqueue(task) {
                        tracing::error!("failed to re-enqueue task: {}", e);
                    }
                    tracing::debug!("failed to reserve capacity for wake");
                    break;
                }

                tracing::info!("woke and started task {}", task.meta.id);

                // Record audit (sync mutex)
                if let Some(audit_sink) = audit.as_ref() {
                    let mut sink = audit_sink.lock();
                    let tenant = task
                        .meta
                        .mailbox
                        .as_ref()
                        .map(|m| m.tenant.clone())
                        .unwrap_or_else(|| "unknown".into());
                    sink.record(crate::core::build_audit_event(
                        format!("{}-wake-{}", task.meta.id, crate::util::clock::now_ms()),
                        task.meta.id.to_string(),
                        "pool",
                        tenant,
                        "wake".to_string(),
                        None,
                    ));
                }

                // Spawn the task
                let executor_clone = executor.clone();
                let queue_clone = Arc::clone(&queue);
                let mailbox_clone = Arc::clone(&mailbox);
                let active_units_clone = Arc::clone(&active_units);
                let wake_condvar_clone = Arc::clone(&wake_condvar);
                let wake_state_clone = Arc::clone(&wake_state);
                let async_wake_enabled_clone = Arc::clone(&async_wake_enabled);
                let limits_clone = limits.clone();
                let audit_clone = audit.clone();
                let spawner_clone = spawner.clone();
                let task_id = task.meta.id;
                let task_cost = task.meta.cost.units;
                let mailbox_key = task.meta.mailbox.clone();
                let meta = task.meta.clone();
                let payload = task.payload;

                spawner.spawn(async move {
                    tracing::debug!("executing woken task {}", task_id);
                    let result = executor_clone.execute(payload, meta).await;
                    tracing::info!("woken task {} completed", task_id);

                    Self::on_task_finished_static(
                        queue_clone,
                        mailbox_clone,
                        active_units_clone,
                        wake_condvar_clone,
                        wake_state_clone,
                        async_wake_enabled_clone,
                        limits_clone,
                        audit_clone,
                        spawner_clone,
                        executor_clone,
                        task_id,
                        task_cost,
                        mailbox_key,
                        result,
                    )
                    .await;
                });
            }
        })
    }

    /// Prune expired tasks from the queue based on current time.
    pub async fn prune_expired(&self, now_ms: u128) -> Result<usize, SchedulerError> {
        let removed = {
            let mut queue = self.queue.lock();
            queue.prune_expired(now_ms)?
        };

        if removed > 0 {
            // Audit generic expiration without specific task IDs (not available after prune).
            if let Some(audit_sink) = &self.audit {
                let mut sink = audit_sink.lock();
                sink.record(crate::core::build_audit_event(
                    format!("expire-batch-{now_ms}"),
                    "batch",
                    "unknown_pool",
                    "unknown_tenant",
                    "expire",
                    None,
                ));
            }
            tracing::warn!("pruned {} expired tasks", removed);
        }
        Ok(removed)
    }

    /// Record an audit event (sync operation with parking_lot mutex).
    fn record_audit(&self, task: &ScheduledTask<P>, action: &str) {
        if let Some(audit_sink) = &self.audit {
            let mut sink = audit_sink.lock();
            let tenant = task
                .meta
                .mailbox
                .as_ref()
                .map(|m| m.tenant.clone())
                .unwrap_or_else(|| "unknown".into());
            sink.record(crate::core::build_audit_event(
                format!("{}-{}-{}", task.meta.id, action, task.meta.created_at_ms),
                task.meta.id.to_string(),
                "pool", // pool name not tracked in metadata; set by caller if desired
                tenant,
                action.to_string(),
                None,
            ));
        }
    }
}

/// Synchronous wake worker that can be run in a dedicated thread.
///
/// This worker waits on the `Condvar` for capacity release notifications and
/// processes queued tasks. Use this instead of async wake tasks for reduced
/// overhead in high-throughput scenarios.
///
/// # Example
///
/// ```ignore
/// use std::thread;
///
/// // Disable async wake and start sync worker
/// pool.async_wake_enabled.store(false, Ordering::Release);
///
/// let queue = Arc::clone(&pool.queue);
/// let mailbox = Arc::clone(&pool.mailbox);
/// let active_units = Arc::clone(&pool.active_units);
/// let wake_condvar = Arc::clone(&pool.wake_condvar);
/// let wake_state = Arc::clone(&pool.wake_state);
/// let limits = pool.limits.clone();
///
/// thread::spawn(move || {
///     sync_wake_worker(queue, mailbox, active_units, wake_condvar, wake_state, limits);
/// });
/// ```
#[allow(dead_code)]
pub fn sync_wake_worker_loop<P, Q>(
    queue: Arc<Mutex<Q>>,
    active_units: Arc<AtomicU32>,
    wake_condvar: Arc<Condvar>,
    wake_state: Arc<Mutex<WakeState>>,
    limits: PoolLimits,
) where
    P: TaskPayload,
    Q: TaskQueue<P>,
{
    loop {
        // Wait for capacity notification
        let mut state = wake_state.lock();
        while !state.capacity_available && !state.shutdown {
            wake_condvar.wait(&mut state);
        }

        if state.shutdown {
            tracing::info!("sync wake worker shutting down");
            break;
        }

        // Reset the flag
        state.capacity_available = false;
        drop(state);

        // Process queued tasks
        loop {
            let task_opt = {
                let mut queue_guard = queue.lock();
                match queue_guard.dequeue() {
                    Ok(task) => task,
                    Err(e) => {
                        tracing::error!("sync wake worker failed to dequeue: {}", e);
                        break;
                    }
                }
            };

            let task = match task_opt {
                Some(t) => t,
                None => {
                    tracing::debug!("sync wake worker: queue empty");
                    break;
                }
            };

            // Try to reserve capacity
            let current = active_units.load(Ordering::Acquire);
            if current + task.meta.cost.units > limits.max_units {
                // Re-enqueue and wait for more capacity
                let mut queue_guard = queue.lock();
                if let Err(e) = queue_guard.enqueue(task) {
                    tracing::error!("sync wake worker failed to re-enqueue: {}", e);
                }
                break;
            }

            // Reserve capacity with CAS
            let mut current = active_units.load(Ordering::Acquire);
            let reserved = loop {
                if current + task.meta.cost.units > limits.max_units {
                    break false;
                }
                match active_units.compare_exchange_weak(
                    current,
                    current + task.meta.cost.units,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break true,
                    Err(actual) => current = actual,
                }
            };

            if !reserved {
                let mut queue_guard = queue.lock();
                if let Err(e) = queue_guard.enqueue(task) {
                    tracing::error!("sync wake worker failed to re-enqueue: {}", e);
                }
                break;
            }

            tracing::info!("sync wake worker: ready to start task {}", task.meta.id);
            // Note: Actual task execution would be handled by passing to executor
            // This worker just reserves capacity and prepares tasks
            // The caller would need to handle the actual execution
        }
    }
}
