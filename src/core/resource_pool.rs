//! Resource pool skeleton and core scheduling traits.

use std::future::Future;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

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

/// Internal state for ResourcePool, wrapped in Arc<Mutex<>> for async mutation.
struct PoolState<P, Q, M> {
    queue: Q,
    mailbox: M,
    active_units: u32,
    _marker: PhantomData<P>,
}

/// Resource pool with capacity accounting and complete parking lot algorithm.
pub struct ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    limits: PoolLimits,
    state: Arc<Mutex<PoolState<P, Q, M>>>,
    executor: E,
    spawner: S,
    audit: Option<Arc<Mutex<Box<dyn AuditSink>>>>,
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
            state: Arc::new(Mutex::new(PoolState {
                queue,
                mailbox,
                active_units: 0,
                _marker: PhantomData,
            })),
            executor,
            spawner,
            audit: None,
            _result_marker: PhantomData,
        }
    }

    /// Attach an audit sink.
    pub fn with_audit(mut self, audit: Box<dyn AuditSink>) -> Self {
        self.audit = Some(Arc::new(Mutex::new(audit)));
        self
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
    /// Returns true if the task fits within available capacity.
    async fn can_start(&self, task: &ScheduledTask<P>) -> bool {
        let state = self.state.lock().await;
        state.active_units + task.meta.cost.units <= self.limits.max_units
    }

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

        let can_start = self.can_start(&task).await;

        if can_start {
            // Reserve capacity
            {
                let mut state = self.state.lock().await;
                state.active_units += task.meta.cost.units;
            }

            // Record audit
            self.record_audit(&task, "start").await;
            tracing::info!("task {} started immediately", task.meta.id);

            // Spawn execution
            self.spawn_task(task).await;

            return Ok(TaskStatus::Running);
        }

        // Not enough capacity - try to enqueue
        let state = self.state.lock().await;
        
        if state.queue.len() >= self.limits.max_queue_depth {
            tracing::warn!(
                "task {} rejected: queue full (depth={})",
                task.meta.id,
                state.queue.len()
            );
            return Err(SchedulerError::QueueFull("max queue depth reached".into()));
        }

        // Record audit before moving task
        drop(state); // Release lock for audit
        self.record_audit(&task, "enqueue").await;
        
        let mut state = self.state.lock().await;
        state.queue.enqueue(task)?;
        tracing::info!("task enqueued");
        Ok(TaskStatus::Queued)
    }

    /// Spawn a task execution asynchronously.
    async fn spawn_task(&self, task: ScheduledTask<P>) {
        let executor = self.executor.clone();
        let state = Arc::clone(&self.state);
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
                state,
                limits,
                audit,
                spawner,
                executor,
                task_id,
                task_cost,
                mailbox_key,
                result,
            ).await;
        });
    }

    /// Static helper for task completion handling (callable from spawned task).
    fn on_task_finished_static(
        state: Arc<Mutex<PoolState<P, Q, M>>>,
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
        // Release capacity
        {
            let mut state_guard = state.lock().await;
            state_guard.active_units = state_guard.active_units.saturating_sub(task_cost);
            tracing::debug!(
                "released {} units, active: {}",
                task_cost,
                state_guard.active_units
            );

            // Deliver to mailbox if key present
            if let Some(ref key) = mailbox_key {
                if let Err(e) = state_guard
                    .mailbox
                    .deliver(key, TaskStatus::Completed, Some(result))
                {
                    tracing::error!("failed to deliver to mailbox: {}", e);
                }
            }
        }

        // Record audit
        if let Some(audit_sink) = audit.as_ref() {
            let mut sink = audit_sink.lock().await;
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

        // Try to wake next task
        let spawner_clone = spawner.clone();
        spawner.spawn(Self::try_wake_next_static(
            state, limits, audit, spawner_clone, executor,
        ));
        })
    }

    /// Try to wake and start the next queued task if capacity available.
    fn try_wake_next_static(
        state: Arc<Mutex<PoolState<P, Q, M>>>,
        limits: PoolLimits,
        audit: Option<Arc<Mutex<Box<dyn AuditSink>>>>,
        spawner: S,
        executor: E,
    ) -> std::pin::Pin<Box<dyn Future<Output = ()> + Send + 'static>> {
        Box::pin(async move {
        loop {
            // Try to dequeue a task
            let task_opt = {
                let mut state_guard = state.lock().await;
                match state_guard.queue.dequeue() {
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

            // Check if we can start this task
            let can_start = {
                let state_guard = state.lock().await;
                state_guard.active_units + task.meta.cost.units <= limits.max_units
            };

            if !can_start {
                // Re-enqueue the task and stop
                let mut state_guard = state.lock().await;
                if let Err(e) = state_guard.queue.enqueue(task) {
                    tracing::error!("failed to re-enqueue task: {}", e);
                }
                tracing::debug!("insufficient capacity to wake next task");
                break;
            }

            // Reserve capacity and spawn
            {
                let mut state_guard = state.lock().await;
                state_guard.active_units += task.meta.cost.units;
            }

            tracing::info!("woke and started task {}", task.meta.id);

            // Record audit
            if let Some(audit_sink) = audit.as_ref() {
                let mut sink = audit_sink.lock().await;
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
            let state_clone = Arc::clone(&state);
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
                    state_clone,
                    limits_clone,
                    audit_clone,
                    spawner_clone,
                    executor_clone,
                    task_id,
                    task_cost,
                    mailbox_key,
                    result,
                ).await;
            });
        }
        })
    }

    /// Prune expired tasks from the queue based on current time.
    pub async fn prune_expired(&self, now_ms: u128) -> Result<usize, SchedulerError> {
        let mut state = self.state.lock().await;
        let removed = state.queue.prune_expired(now_ms)?;
        
        if removed > 0 {
            // Audit generic expiration without specific task IDs (not available after prune).
            if let Some(audit_sink) = &self.audit {
                let mut sink = audit_sink.lock().await;
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

    async fn record_audit(&self, task: &ScheduledTask<P>, action: &str) {
        if let Some(audit_sink) = &self.audit {
            let mut sink = audit_sink.lock().await;
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
