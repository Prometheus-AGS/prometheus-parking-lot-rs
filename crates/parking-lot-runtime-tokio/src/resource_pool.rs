//! Resource pool: the async parking-lot scheduler built on the core contracts.
//!
//! All shared state lives in one `Inner` behind one `Arc`. A running task's
//! future owns a clone of that `Arc`, so completing a task (release capacity,
//! deliver the outcome, start the next queued task) needs no per-field `Arc`s
//! threaded through boxed futures.
//!
//! ## Admission and ordering
//!
//! Capacity is claimed only while the queue lock is held, and a submission
//! starts immediately only when the queue is empty. Queued tasks therefore
//! start strictly in queue order: highest priority first, FIFO within a
//! priority by the submission sequence the pool stamps on each task. A task
//! whose cost exceeds the pool's total capacity is rejected at submission,
//! because parked at the head of the queue it would block every task behind
//! it forever.

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

use prometheus_parking_lot_core::{
    AuditSink, Clock, Mailbox, MailboxKey, PoolLimits, ScheduledTask, SchedulerError, Spawn,
    SubmissionSequence, SystemClock, TaskError, TaskExecutor, TaskId, TaskMetadata, TaskPayload,
    TaskQueue, TaskStatus,
};

/// Pool name used in errors and audit events (pools are not named yet).
const POOL_NAME: &str = "resource_pool";

/// Resource pool with capacity accounting and the parking-lot algorithm.
///
/// Tasks start immediately while capacity is free and the queue is empty;
/// otherwise they are parked in the queue and started, in priority-then-FIFO
/// order, as running tasks release capacity. Every execution is bounded by
/// its deadline or the pool's `default_timeout`, and its outcome — the value,
/// or the [`TaskError`] — is delivered to the mailbox when the task has a
/// mailbox key.
///
/// Time is read through a [`Clock`] ([`SystemClock`] by default; see
/// [`with_clock`](Self::with_clock)).
pub struct ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    inner: Arc<Inner<P, T, Q, M, E, S>>,
}

/// State shared by the pool handle and every running task.
struct Inner<P, T, Q, M, E, S> {
    limits: PoolLimits,
    /// Resource units held by running tasks. Claimed only under the queue
    /// lock; released without it.
    active_units: AtomicU32,
    queue: Mutex<Q>,
    mailbox: Mutex<M>,
    executor: E,
    spawner: S,
    audit: Mutex<Option<Box<dyn AuditSink>>>,
    clock: RwLock<Arc<dyn Clock>>,
    sequence: SubmissionSequence,
    shutdown: AtomicBool,
    /// `fn() -> _` keeps the pool `Send`/`Sync` independent of `P` and `T`,
    /// which it never stores.
    _types: PhantomData<fn() -> (P, T)>,
}

impl<P, T, Q, M, E, S> ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    /// Create a new pool from components.
    pub fn new(limits: PoolLimits, queue: Q, mailbox: M, executor: E, spawner: S) -> Self {
        Self {
            inner: Arc::new(Inner {
                limits,
                active_units: AtomicU32::new(0),
                queue: Mutex::new(queue),
                mailbox: Mutex::new(mailbox),
                executor,
                spawner,
                audit: Mutex::new(None),
                clock: RwLock::new(Arc::new(SystemClock::new())),
                sequence: SubmissionSequence::new(),
                shutdown: AtomicBool::new(false),
                _types: PhantomData,
            }),
        }
    }

    /// Attach an audit sink.
    #[must_use]
    pub fn with_audit(self, audit: Box<dyn AuditSink>) -> Self {
        *self.inner.audit.lock() = Some(audit);
        self
    }

    /// Read time from `clock` instead of the system clock: deadline checks,
    /// execution budgets, queue pruning and audit timestamps all use it.
    ///
    /// Pass a [`TestClock`](prometheus_parking_lot_core::TestClock) clone to
    /// drive deadlines from a test.
    #[must_use]
    pub fn with_clock(self, clock: impl Clock + 'static) -> Self {
        *self.inner.clock.write() = Arc::new(clock);
        self
    }

    /// Resource units currently held by running tasks.
    #[must_use]
    pub fn active_units(&self) -> u32 {
        self.inner.active_units.load(Ordering::Acquire)
    }

    /// Stop accepting new submissions.
    ///
    /// Later calls to [`submit`](Self::submit) return
    /// [`SchedulerError::PoolShutdown`]. Running tasks finish and queued tasks
    /// keep draining as capacity frees; draining and cancellation policy is
    /// cr-29.
    pub fn shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::Release);
    }
}

impl<P, T, Q, M, E, S> ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    Q: TaskQueue<P> + Send + 'static,
    M: Mailbox<T> + Send + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Send + Sync + 'static,
{
    /// Submit a task, enforcing deadline, capacity and queue depth.
    ///
    /// The pool stamps the task's [`TaskMetadata::sequence`]. The task starts
    /// immediately when the queue is empty and its cost fits the free
    /// capacity (returns [`TaskStatus::Running`]); otherwise it is parked
    /// (returns [`TaskStatus::Queued`]).
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::PoolShutdown`] after [`shutdown`](Self::shutdown);
    /// - [`SchedulerError::DeadlineExpired`] when the task's deadline has
    ///   already passed by the pool's clock;
    /// - [`SchedulerError::CapacityExceeded`] when the task's cost exceeds the
    ///   pool's total capacity, so it could never start;
    /// - [`SchedulerError::QueueFull`] when the task must wait and the queue is
    ///   at its depth limit;
    /// - the queue backend's error when enqueueing fails.
    #[expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "public async API; dispatch becomes awaited with the durable dispatcher (cr-18/cr-49), so the signature stays async (the trait-impl variant misfires on an inherent method)"
    )]
    pub async fn submit(&self, mut task: ScheduledTask<P>) -> Result<TaskStatus, SchedulerError> {
        let inner = &self.inner;
        if inner.shutdown.load(Ordering::Acquire) {
            return Err(SchedulerError::PoolShutdown {
                pool: POOL_NAME.into(),
            });
        }
        if inner.is_expired(&task.meta) {
            tracing::warn!(task_id = task.meta.id, "task expired before enqueue");
            return Err(SchedulerError::DeadlineExpired {
                task_id: task.meta.id,
            });
        }
        let cost = task.meta.cost.total_units();
        if cost > inner.limits.max_units {
            return Err(SchedulerError::CapacityExceeded {
                pool: POOL_NAME.into(),
                requested: cost,
                available: inner.limits.max_units,
            });
        }

        let mut queue = inner.queue.lock();
        task.meta.sequence = inner.sequence.next();
        if queue.is_empty() && inner.try_reserve(cost) {
            drop(queue);
            inner.record_audit(&task.meta, "start");
            tracing::debug!(task_id = task.meta.id, "task started immediately");
            inner.start(task);
            return Ok(TaskStatus::Running);
        }

        let depth = queue.len();
        if depth >= inner.limits.max_queue_depth {
            tracing::warn!(task_id = task.meta.id, depth, "task rejected: queue full");
            return Err(SchedulerError::queue_full(
                POOL_NAME,
                depth,
                inner.limits.max_queue_depth,
            ));
        }
        // Audited under the queue lock so the "enqueue" event precedes the
        // "wake" event of the dispatch that will start this task.
        inner.record_audit(&task.meta, "enqueue");
        queue.enqueue(task)?;
        drop(queue);
        tracing::debug!("task enqueued");
        Ok(TaskStatus::Queued)
    }

    /// Remove queued tasks whose deadline has passed by the pool's clock and
    /// return how many were removed.
    ///
    /// Queued tasks are also checked when they reach the head of the queue: a
    /// task found expired there is never started and its mailbox receives
    /// [`TaskStatus::Expired`].
    ///
    /// # Errors
    ///
    /// Returns the queue backend's error when pruning fails.
    #[expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "public async API; async queue backends (cr-25) make pruning awaited, so the signature stays async (the trait-impl variant misfires on an inherent method)"
    )]
    pub async fn prune_expired(&self) -> Result<usize, SchedulerError> {
        let inner = &self.inner;
        let now_ms = inner.now_ms();
        let removed = inner.queue.lock().prune_expired(now_ms)?;

        if removed > 0 {
            // Generic expiration event: the pruned task ids are not reported
            // by the queue contract.
            if let Some(sink) = inner.audit.lock().as_mut() {
                sink.record(prometheus_parking_lot_core::build_audit_event(
                    format!("expire-batch-{now_ms}"),
                    "batch",
                    "unknown_pool",
                    "unknown_tenant",
                    "expire",
                    None,
                ));
            }
            tracing::warn!(removed, "pruned expired tasks");
        }
        Ok(removed)
    }
}

impl<P, T, Q, M, E, S> Inner<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    Q: TaskQueue<P> + Send + 'static,
    M: Mailbox<T> + Send + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Send + Sync + 'static,
{
    fn now_ms(&self) -> u128 {
        self.clock.read().now_ms()
    }

    fn is_expired(&self, meta: &TaskMetadata) -> bool {
        meta.deadline_ms
            .is_some_and(|deadline| self.now_ms() > deadline)
    }

    /// Claim `cost` units if they fit. Callers hold the queue lock.
    fn try_reserve(&self, cost: u32) -> bool {
        let max = self.limits.max_units;
        let mut active = self.active_units.load(Ordering::Acquire);
        loop {
            let Some(next) = active.checked_add(cost).filter(|&next| next <= max) else {
                return false;
            };
            match self.active_units.compare_exchange_weak(
                active,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(current) => active = current,
            }
        }
    }

    /// Resolve the execution budget for one task.
    ///
    /// A per-task `deadline_ms` wins when it is nearer than the pool's
    /// `default_timeout`; otherwise the default applies. Every task is bounded:
    /// no path returns an unbounded budget, which is what keeps the capacity
    /// guarantee under a wedged executor. An already-passed deadline yields a
    /// zero budget, so the task fails fast.
    fn budget(&self, meta: &TaskMetadata) -> Duration {
        let Some(deadline) = meta.deadline_ms else {
            return self.limits.default_timeout;
        };
        let remaining = deadline.saturating_sub(self.now_ms());
        Duration::from_millis(u64::try_from(remaining).unwrap_or(u64::MAX))
            .min(self.limits.default_timeout)
    }

    /// Spawn a task whose capacity is already reserved.
    fn start(self: &Arc<Self>, task: ScheduledTask<P>) {
        let inner = Arc::clone(self);
        self.spawner.spawn(inner.run(task));
    }

    /// Execute one task under its budget, then finish it.
    async fn run(self: Arc<Self>, task: ScheduledTask<P>) {
        let ScheduledTask { meta, payload } = task;
        let task_id = meta.id;
        let cost = meta.cost.total_units();
        let mailbox_key = meta.mailbox.clone();
        let budget = self.budget(&meta);
        tracing::debug!(task_id, ?budget, "executing task");

        // On elapse the executor future is dropped, which cancels it at its
        // next await point; an executor owning a `tokio::process::Child` built
        // with `kill_on_drop(true)` has the child reaped by that drop.
        let outcome = tokio::time::timeout(budget, self.executor.execute(payload, meta))
            .await
            .unwrap_or(Err(TaskError::Timeout { budget }));

        self.finish(task_id, cost, mailbox_key.as_ref(), outcome);
    }

    /// Release a finished task's capacity, deliver its outcome, audit it and
    /// start whatever queued work now fits.
    fn finish(
        self: &Arc<Self>,
        task_id: TaskId,
        cost: u32,
        mailbox_key: Option<&MailboxKey>,
        outcome: Result<T, TaskError>,
    ) {
        self.active_units.fetch_sub(cost, Ordering::AcqRel);

        let action = match &outcome {
            Ok(_) => "complete",
            Err(TaskError::Timeout { budget }) => {
                tracing::warn!(
                    task_id,
                    ?budget,
                    "task exceeded its budget and was cancelled"
                );
                "timeout"
            }
            Err(error) => {
                tracing::warn!(task_id, %error, "task failed");
                "fail"
            }
        };

        if let Some(key) = mailbox_key {
            let (status, payload) = match outcome {
                Ok(value) => (TaskStatus::Completed, Some(value)),
                Err(error) => (TaskStatus::Failed(error), None),
            };
            self.deliver(task_id, key, status, payload);
        }

        let tenant = mailbox_key.map_or("unknown", |key| key.tenant.as_str());
        self.record_event(task_id, tenant, action);
        self.dispatch_queued();
    }

    /// Start queued tasks, in queue order, while the head of the queue fits.
    ///
    /// The head is dequeued, checked and, if it does not fit, put back while
    /// the queue lock is held, so no submission can slip in front of it; the
    /// sequence number it carries restores its exact place.
    fn dispatch_queued(self: &Arc<Self>) {
        loop {
            let mut queue = self.queue.lock();
            let task = match queue.dequeue() {
                Ok(Some(task)) => task,
                Ok(None) => return,
                Err(error) => {
                    tracing::error!(%error, "failed to dequeue");
                    return;
                }
            };

            if self.is_expired(&task.meta) {
                drop(queue);
                self.expire(&task.meta);
                continue;
            }

            if !self.try_reserve(task.meta.cost.total_units()) {
                let task_id = task.meta.id;
                if let Err(error) = queue.enqueue(task) {
                    tracing::error!(task_id, %error, "failed to re-enqueue task; it is lost");
                }
                return;
            }
            drop(queue);

            tracing::debug!(task_id = task.meta.id, "woke queued task");
            self.record_audit(&task.meta, "wake");
            self.start(task);
        }
    }

    /// Record a queued task that reached the head after its deadline.
    fn expire(&self, meta: &TaskMetadata) {
        tracing::warn!(
            task_id = meta.id,
            "queued task expired before it could start"
        );
        if let Some(key) = meta.mailbox.as_ref() {
            self.deliver(meta.id, key, TaskStatus::Expired, None);
        }
        let tenant = meta
            .mailbox
            .as_ref()
            .map_or("unknown", |key| key.tenant.as_str());
        self.record_event(meta.id, tenant, "expire");
    }

    /// Deliver an outcome to the mailbox. A failed delivery is logged, not
    /// propagated: the task has already finished and its capacity is released.
    fn deliver(&self, task_id: TaskId, key: &MailboxKey, status: TaskStatus, payload: Option<T>) {
        let delivered = self.mailbox.lock().deliver(key, status, payload);
        if let Err(error) = delivered {
            tracing::error!(task_id, %error, "failed to deliver to mailbox");
        }
    }

    /// Record a lifecycle audit event keyed by the task's creation time.
    fn record_audit(&self, meta: &TaskMetadata, action: &str) {
        if let Some(sink) = self.audit.lock().as_mut() {
            let tenant = meta
                .mailbox
                .as_ref()
                .map_or_else(|| "unknown".into(), |m| m.tenant.clone());
            sink.record(prometheus_parking_lot_core::build_audit_event(
                format!("{}-{}-{}", meta.id, action, meta.created_at_ms),
                meta.id.to_string(),
                "pool",
                tenant,
                action.to_string(),
                None,
            ));
        }
    }

    /// Record an audit event keyed by the pool clock's current time.
    fn record_event(&self, task_id: TaskId, tenant: &str, action: &str) {
        if let Some(sink) = self.audit.lock().as_mut() {
            sink.record(prometheus_parking_lot_core::build_audit_event(
                format!("{task_id}-{action}-{}", self.now_ms()),
                task_id.to_string(),
                "pool",
                tenant.to_string(),
                action.to_string(),
                None,
            ));
        }
    }
}
