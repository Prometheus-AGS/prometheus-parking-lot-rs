//! Resource pool: the async parking-lot scheduler built on the core contracts.
//!
//! All shared state lives in one `Inner` behind one `Arc`. A running task's
//! future owns a clone of that `Arc`, so completing a task (release capacity,
//! deliver the outcome, start the next queued task) needs no per-field `Arc`s
//! threaded through boxed futures.
//!
//! ## Admission and ordering
//!
//! Admission, parking and waking go through the core
//! [`Dispatcher`], which
//! owns the unit-weighted semaphore and the parked queue (the configured
//! [`TaskQueue`] backend behind a
//! [`TaskQueueAdapter`]).
//! A submission starts immediately only when nothing is parked and its cost
//! fits; parked tasks start strictly in queue order — highest priority first,
//! FIFO within a priority by the submission sequence the dispatcher stamps —
//! and a head that does not fit is never taken out of the queue. A task whose
//! cost exceeds the pool's total capacity is rejected at submission, because
//! parked at the head of the queue it would block every task behind it
//! forever.
//!
//! A running task owns its semaphore permit; completion returns the permit
//! through the dispatcher, which starts whatever the freed units admit under
//! the same lock (no lost wakeup; see the dispatcher's module docs).
//!
//! ## Panics
//!
//! An executor panic is contained (cr-21): the task fails with
//! [`TaskError::ExecutorPanicked`], its units are returned through the
//! dispatcher like any other outcome and the pool keeps serving. A panic that
//! gets past that containment (see `crate::contain`) is caught by a backstop
//! around the whole task: the permit, freed by unwinding, is followed by
//! [`Dispatcher::poke`] so parked work it admits starts at once, and the task
//! is reported as panicked unless its outcome was already recorded.

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};

use crate::contain::contain;

use prometheus_parking_lot_core::dispatch::{Dispatched, Dispatcher, Grant, TaskQueueAdapter};
use prometheus_parking_lot_core::semaphore::{Permit, UnitSemaphore};
use prometheus_parking_lot_core::{
    AuditSink, Clock, Mailbox, MailboxKey, PoolLimits, ScheduledTask, SchedulerError, Spawn,
    SystemClock, TaskError, TaskExecutor, TaskId, TaskMetadata, TaskPayload, TaskQueue, TaskStatus,
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
    /// Owns the semaphore (units held by running tasks) and the parked queue.
    dispatcher: Dispatcher<ScheduledTask<P>, TaskQueueAdapter<P, Q>>,
    mailbox: Mutex<M>,
    executor: E,
    spawner: S,
    audit: Mutex<Option<Box<dyn AuditSink>>>,
    clock: RwLock<Arc<dyn Clock>>,
    shutdown: AtomicBool,
    /// Tasks that ran and returned a value.
    completed: AtomicU64,
    /// Tasks whose outcome was a failure: an executor error or panic, a
    /// timeout, or a deadline that passed while parked.
    failed: AtomicU64,
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
    ///
    /// `queue` stores the parked tasks; `limits.max_queue_depth` bounds how
    /// many may be parked (a backend with a smaller own limit rejects earlier).
    pub fn new(limits: PoolLimits, queue: Q, mailbox: M, executor: E, spawner: S) -> Self {
        let dispatcher = Dispatcher::new(
            POOL_NAME,
            UnitSemaphore::new(limits.max_units),
            limits.max_queue_depth,
            TaskQueueAdapter::new(queue),
        );
        Self {
            inner: Arc::new(Inner {
                limits,
                dispatcher,
                mailbox: Mutex::new(mailbox),
                executor,
                spawner,
                audit: Mutex::new(None),
                clock: RwLock::new(Arc::new(SystemClock::new())),
                shutdown: AtomicBool::new(false),
                completed: AtomicU64::new(0),
                failed: AtomicU64::new(0),
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
        self.inner.dispatcher.units_in_use()
    }

    /// Tasks currently running (holding units).
    #[must_use]
    pub fn running_tasks(&self) -> u32 {
        self.inner.dispatcher.running()
    }

    /// Tasks that ran and returned a value.
    ///
    /// Counted before the outcome is delivered, so a caller that has read a
    /// task's outcome from the mailbox sees it counted.
    #[must_use]
    pub fn completed_tasks(&self) -> u64 {
        // Relaxed: a statistic. The increment is sequenced before the
        // delivery's mailbox unlock, so a reader ordered after the delivery
        // (through the mailbox's lock) observes it regardless.
        self.inner.completed.load(Ordering::Relaxed)
    }

    /// Tasks whose outcome was a failure: an executor error or panic
    /// ([`TaskError::ExecutorPanicked`]), a timeout, or a deadline that passed
    /// while the task was parked. Counted before delivery, like
    /// [`completed_tasks`](Self::completed_tasks).
    #[must_use]
    pub fn failed_tasks(&self) -> u64 {
        // Relaxed: as in `completed_tasks`.
        self.inner.failed.load(Ordering::Relaxed)
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
    /// Tasks currently parked, waiting for capacity (at most
    /// `max_queue_depth`).
    #[must_use]
    pub fn queued_tasks(&self) -> usize {
        self.inner.dispatcher.parked()
    }

    /// Submit a task, enforcing deadline, capacity and queue depth.
    ///
    /// The pool stamps the task's [`TaskMetadata::sequence`]. The task starts
    /// immediately when nothing is parked and its cost fits the free
    /// capacity (returns [`TaskStatus::Running`]); otherwise it is parked
    /// (returns [`TaskStatus::Queued`], or [`TaskStatus::Running`] when the
    /// parked task was at the head and fit at once). Parked tasks this call
    /// admits are started too.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::PoolShutdown`] after [`shutdown`](Self::shutdown);
    /// - [`SchedulerError::DeadlineExpired`] when the task's deadline has
    ///   already passed by the pool's clock;
    /// - [`SchedulerError::CapacityExceeded`] when the task's cost exceeds the
    ///   pool's total capacity, so it could never start;
    /// - [`SchedulerError::QueueFull`] when the task must wait and the queue is
    ///   at its depth limit (checked atomically with the enqueue);
    /// - the queue backend's error when enqueueing fails.
    #[expect(
        clippy::unused_async,
        clippy::unused_async_trait_impl,
        reason = "public async API; dispatch becomes awaited with the durable dispatcher (cr-18/cr-49), so the signature stays async (the trait-impl variant misfires on an inherent method)"
    )]
    pub async fn submit(&self, task: ScheduledTask<P>) -> Result<TaskStatus, SchedulerError> {
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
        let now_ms = inner.now_ms();
        let meta = task.meta.clone();
        let admission = inner.dispatcher.submit(task, now_ms).inspect_err(|error| {
            tracing::warn!(task_id = meta.id, %error, "task rejected at admission");
        })?;
        let status = if admission.started {
            TaskStatus::Running
        } else {
            // Recorded after the dispatch lock is released, so a completion
            // racing this submission can audit the task's "wake" first.
            tracing::debug!(task_id = meta.id, "task parked");
            inner.record_audit(&meta, "enqueue");
            TaskStatus::Queued
        };
        inner.launch(admission.dispatched, Some(admission.sequence));
        Ok(status)
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
        let (removed, dispatched) = inner.dispatcher.prune_expired(now_ms)?;
        // Removing an expired head that did not fit can admit the tasks
        // behind it.
        inner.launch(dispatched, None);

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

    /// Resolve the execution budget for one task.
    ///
    /// A per-task `deadline_ms` wins when it is nearer than the pool's
    /// `default_timeout`; otherwise the default applies. Every task is bounded:
    /// no path returns an unbounded budget, which is what keeps the capacity
    /// guarantee under a wedged executor. An already-passed deadline yields a
    /// zero budget, so the task fails fast.
    #[cfg(not(parking_lot_negative_control = "s-4"))]
    fn budget(&self, meta: &TaskMetadata) -> Duration {
        let Some(deadline) = meta.deadline_ms else {
            return self.limits.default_timeout;
        };
        let remaining = deadline.saturating_sub(self.now_ms());
        Duration::from_millis(u64::try_from(remaining).unwrap_or(u64::MAX))
            .min(self.limits.default_timeout)
    }

    /// Negative control S-4: the budget is not applied (effectively
    /// unbounded), so the `budget_` wedged-executor tests must fail.
    #[cfg(parking_lot_negative_control = "s-4")]
    fn budget(&self, meta: &TaskMetadata) -> Duration {
        let _ = (meta, &self.limits);
        Duration::from_secs(u64::from(u32::MAX))
    }

    /// Start every task the dispatcher admitted and report every task it
    /// found expired at the head. `submitted` is the sequence of the task
    /// whose submission produced `dispatched`, which is audited as "start"
    /// rather than "wake".
    fn launch(self: &Arc<Self>, dispatched: Dispatched<ScheduledTask<P>>, submitted: Option<u64>) {
        let Dispatched { started, expired } = dispatched;
        for meta in expired.iter().map(|task| &task.meta) {
            self.expire(meta);
        }
        for Grant { item: task, permit } in started {
            let action = if submitted == Some(task.meta.sequence) {
                "start"
            } else {
                "wake"
            };
            tracing::debug!(task_id = task.meta.id, action, "starting task");
            self.record_audit(&task.meta, action);
            let inner = Arc::clone(self);
            self.spawner.spawn(inner.supervise(task, permit));
        }
    }

    /// Run one task behind the panic backstop.
    ///
    /// [`run`](Self::run) contains executor panics itself. A panic that still
    /// escapes it (a payload whose destructor panics, or a panicking `Clock`,
    /// `Mailbox` or `AuditSink` in the completion path) is caught here: the
    /// `run` future, and with it the task's permit, has been dropped by then,
    /// so the freed units are handed to parked work with
    /// [`Dispatcher::poke`]. A task whose outcome was not recorded yet is
    /// reported as panicked; one whose completion had started is not reported
    /// twice. If the panic struck after the completion took woken tasks off
    /// the queue but before they were started, those tasks are lost with it;
    /// that needs a panicking `Mailbox` or `AuditSink`.
    async fn supervise(self: Arc<Self>, task: ScheduledTask<P>, permit: Permit) {
        let task_id = task.meta.id;
        let mailbox_key = task.meta.mailbox.clone();
        // Set by `run` just before it starts the completion. Relaxed: it is
        // written and read by this one task; tokio orders its polls.
        let reported = AtomicBool::new(false);
        let ran = contain(Arc::clone(&self).run(task, permit, &reported)).await;
        if let Err(panicked) = ran {
            tracing::error!(
                task_id,
                %panicked,
                "panic escaped the executor containment"
            );
            if !reported.load(Ordering::Relaxed) {
                self.failed.fetch_add(1, Ordering::Relaxed);
                if let Some(key) = mailbox_key.as_ref() {
                    let status = TaskStatus::Failed(TaskError::from(panicked));
                    self.deliver(task_id, key, status, None);
                }
                let tenant = mailbox_key
                    .as_ref()
                    .map_or("unknown", |key| key.tenant.as_str());
                self.record_event(task_id, tenant, "fail");
            }
            let woken = self.dispatcher.poke(self.now_ms());
            self.launch(woken, None);
        }
    }

    /// Execute one task under its budget, then finish it.
    ///
    /// The executor call, its future and the budget timer run inside one
    /// [`contain`], so an executor panic — in the call, in a poll, or in the
    /// drop of its future when the budget elapses — becomes
    /// [`TaskError::ExecutorPanicked`] and the task finishes normally.
    async fn run(self: Arc<Self>, task: ScheduledTask<P>, permit: Permit, reported: &AtomicBool) {
        let ScheduledTask { meta, payload } = task;
        let task_id = meta.id;
        let mailbox_key = meta.mailbox.clone();
        let budget = self.budget(&meta);
        tracing::debug!(task_id, ?budget, "executing task");

        // On elapse the executor future is dropped, which cancels it at its
        // next await point; an executor owning a `tokio::process::Child` built
        // with `kill_on_drop(true)` has the child reaped by that drop.
        let executor = &self.executor;
        let contained =
            contain(
                async move { tokio::time::timeout(budget, executor.execute(payload, meta)).await },
            )
            .await;
        let outcome = match contained {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(_elapsed)) => Err(TaskError::Timeout { budget }),
            Err(panicked) => Err(TaskError::from(panicked)),
        };

        reported.store(true, Ordering::Relaxed);
        self.finish(task_id, permit, mailbox_key.as_ref(), outcome);
    }

    /// Release a finished task's capacity, deliver its outcome, audit it and
    /// start whatever queued work now fits.
    ///
    /// The permit is returned (and the woken tasks taken off the queue) before
    /// the outcome is delivered, so a caller that reads the outcome from the
    /// mailbox already sees the units released. The woken tasks are spawned
    /// after delivery, outside the dispatch lock.
    fn finish(
        self: &Arc<Self>,
        task_id: TaskId,
        permit: Permit,
        mailbox_key: Option<&MailboxKey>,
        outcome: Result<T, TaskError>,
    ) {
        let woken = self.dispatcher.complete(permit, self.now_ms());
        let counter = if outcome.is_ok() {
            &self.completed
        } else {
            &self.failed
        };
        // Relaxed: see `ResourcePool::completed_tasks`.
        counter.fetch_add(1, Ordering::Relaxed);
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
            Err(error @ TaskError::ExecutorPanicked { .. }) => {
                tracing::error!(task_id, %error, "executor panicked; task failed");
                "fail"
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
        self.launch(woken, None);
    }

    /// Record a queued task that reached the head after its deadline.
    fn expire(&self, meta: &TaskMetadata) {
        self.failed.fetch_add(1, Ordering::Relaxed);
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
