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
//! fits; parked tasks start in queue order — highest priority first, FIFO
//! within a priority by the submission sequence the dispatcher stamps — and a
//! head that does not fit is never taken out of the queue. While the head does
//! not fit, a parked task behind it that does may start in its place
//! (skip-ahead, cr-20), at most
//! [`max_head_skips`](ResourcePool::with_max_head_skips) times per head; then
//! the pool reserves capacity for the head. The lookup behind the head sees
//! the first
//! [`ADAPTER_LOOKAHEAD`](prometheus_parking_lot_core::dispatch::ADAPTER_LOOKAHEAD)
//! parked tasks. A task whose cost exceeds the
//! pool's total capacity is rejected at submission, because parked at the head
//! of the queue it would block every task behind it forever.
//!
//! With per-kind limits ([`with_resource_limit`](ResourcePool::with_resource_limit),
//! cr-23) a task's cost is a vector: it starts only when it fits the total
//! `max_units` and every limited kind at once (a task needing 8 GiB of VRAM
//! and one slot waits while 8 GiB are free but no slot is), and a task over
//! any one limit is rejected at submission.
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
//!
//! ## Shutdown (cr-29)
//!
//! [`shutdown_with`](ResourcePool::shutdown_with) applies a
//! [`ShutdownPolicy`]: queued tasks keep starting during the drain, running
//! tasks finish, and at the drain deadline the queued remainder (and, unless
//! the policy is `Drain`, the running stragglers) are cancelled: the mailbox
//! receives [`TaskError::Cancelled`] and a [`CancelledTask`] goes to the
//! pool's [`CancellationSink`]. A running straggler's payload is restored from
//! a JSON snapshot the pool keeps while a sink is registered. The pool's
//! ledger counts accepted tasks until each is settled, so the returned
//! [`ShutdownReport`] accounts for every one of them.
//!
//! ## Lifecycle events (cr-31)
//!
//! Hooks registered with [`with_hook`](ResourcePool::with_hook) (or on
//! [`hooks`](ResourcePool::hooks)) observe, per task, keyed by
//! [`TaskMetadata::id`]:
//!
//! | event | emitted |
//! |---|---|
//! | `Submitted` | first thing in `submit`, before any check |
//! | `Rejected` | `submit` refuses the task (shutdown, deadline passed, cost over capacity, queue full, backend error) |
//! | `Queued` | the dispatcher parks it, under the dispatch lock |
//! | `Started` | the spawned task is about to call the executor (`worker_id` 0) |
//! | `Completed` / `Failed` | the executor returned a value / an error, panicked or ran past its budget (before the units are returned) |
//! | `Expired` | it reached the head of the queue after its deadline |
//! | `Cancelled` | `shutdown_with` cancelled it (queued: no `Started`; running straggler) |
//!
//! A panic that escapes the executor containment reports `Failed` from the
//! backstop unless the terminal event was already emitted. Not covered:
//! tasks removed by [`prune_expired`](ResourcePool::prune_expired), whose ids
//! the `TaskQueue` contract does not return (they get no terminal event, as
//! they get no mailbox delivery).

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::{Mutex, RwLock};
use tokio_util::sync::CancellationToken;

use crate::cancellation::{
    CancelReason, CancellationConfig, CancellationSink, CancelledTask, LoggingCancellationSink,
    offer,
};
use crate::contain::contain;
use crate::events::Events;
use crate::ledger::{Ledger, SettleOnDrop};
use crate::shutdown::{ShutdownPolicy, ShutdownReport};

use prometheus_parking_lot_core::dispatch::{
    Dispatched, Dispatcher, Grant, SkipAheadStats, TaskQueueAdapter,
};
use prometheus_parking_lot_core::semaphore::{Permit, UnitSemaphore};
use prometheus_parking_lot_core::{
    AuditSink, Clock, Hook, HookBus, Mailbox, MailboxKey, PoolLimits, ResourceKind, ScheduledTask,
    SchedulerError, Spawn, SystemClock, TaskError, TaskExecutor, TaskId, TaskMetadata, TaskPayload,
    TaskQueue, TaskStatus,
};

/// Pool name used in errors, lifecycle events and logs until
/// [`ResourcePool::with_name`] sets one.
const POOL_NAME: &str = "resource_pool";

/// Extra wait, beyond the sink's `delta`, for cancelled stragglers to settle.
const SETTLE_MARGIN: Duration = Duration::from_secs(1);

/// The error a cancelled task's mailbox receives.
fn shutdown_cancelled() -> TaskError {
    TaskError::Cancelled {
        reason: "resource pool shut down".into(),
    }
}

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
    /// Set by `shutdown`/`shutdown_with`: submissions are refused.
    shutdown: AtomicBool,
    /// Set when `shutdown_with` takes the parked queue at the drain deadline:
    /// a submission that raced it captures what it parked.
    parked_closed: AtomicBool,
    /// Fired at the drain deadline (policies that cancel stragglers): running
    /// tasks stop at their next await point and are captured.
    cancel: CancellationToken,
    /// Accepted tasks until settled, and how cancellations ended.
    ledger: Ledger,
    /// Where cancelled tasks go; `None` means a [`LoggingCancellationSink`]
    /// and no payload snapshots.
    sink: RwLock<Option<Arc<dyn CancellationSink<P>>>>,
    /// The sink's per-call bound (`delta`).
    cancellation: RwLock<CancellationConfig>,
    /// Lifecycle-event observers (cr-31).
    hooks: HookBus,
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
                parked_closed: AtomicBool::new(false),
                cancel: CancellationToken::new(),
                ledger: Ledger::default(),
                sink: RwLock::new(None),
                cancellation: RwLock::new(CancellationConfig::default()),
                hooks: HookBus::new(),
                completed: AtomicU64::new(0),
                failed: AtomicU64::new(0),
                _types: PhantomData,
            }),
        }
    }

    /// Name the pool: the name appears in its errors
    /// ([`SchedulerError::CapacityExceeded`], `QueueFull`, `PoolShutdown`),
    /// its lifecycle events and its logs. Default: `"resource_pool"`. Pools
    /// built from a configuration file take their key in `pools`.
    ///
    /// # Panics
    ///
    /// After the first `submit`, even once the pool is idle again: the first
    /// submission installs the dispatcher's release hook, which keeps a weak
    /// reference to the pool's state, so the state is never exclusively owned
    /// again. The name is part of building the pool; call it before the
    /// first `submit`.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        let Some(inner) = Arc::get_mut(&mut self.inner) else {
            panic!("ResourcePool::with_name must be called before the pool holds work");
        };
        inner.dispatcher.set_pool_name(name);
        self
    }

    /// The pool's name (see [`with_name`](Self::with_name)).
    #[must_use]
    pub fn name(&self) -> &str {
        self.inner.dispatcher.pool_name()
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

    /// How many times a parked head that does not fit may be skipped by
    /// smaller tasks that do before the pool reserves capacity for it (cr-20).
    /// `0` keeps strict head-of-line order. Default:
    /// [`DEFAULT_MAX_HEAD_SKIPS`](prometheus_parking_lot_core::dispatch::DEFAULT_MAX_HEAD_SKIPS).
    #[must_use]
    pub fn with_max_head_skips(self, max_head_skips: u32) -> Self {
        self.inner.dispatcher.set_max_head_skips(max_head_skips);
        self
    }

    /// Limit the units of dimension `kind` held by running tasks to `limit`
    /// (cr-23). A task is then admitted only when its cost fits
    /// `limits.max_units` (the total over every kind) and every limited kind at
    /// once; a kind without a limit is bounded by the total alone. A task
    /// needing more than `limit` of `kind` is rejected with
    /// [`SchedulerError::CapacityExceeded`].
    ///
    /// # Panics
    ///
    /// After the first `submit`, even once the pool is idle again (the
    /// dispatcher refuses new limits once it has seen a submission, and the
    /// release hook the first submission installs keeps the pool's state
    /// shared): limits are part of building the pool. Call it before the
    /// first `submit`.
    #[must_use]
    pub fn with_resource_limit(mut self, kind: ResourceKind, limit: u32) -> Self {
        let set = Arc::get_mut(&mut self.inner)
            .ok_or_else(|| {
                SchedulerError::invalid_config("resource pool is shared by running tasks")
            })
            .and_then(|inner| inner.dispatcher.set_resource_limit(kind, limit));
        if let Err(error) = set {
            panic!(
                "ResourcePool::with_resource_limit must be called before the pool holds work: {error}"
            );
        }
        self
    }

    /// Skip-ahead counters: tasks started ahead of a blocked head, and heads
    /// the starvation guard reserved capacity for.
    #[must_use]
    pub fn skip_ahead_stats(&self) -> SkipAheadStats {
        self.inner.dispatcher.skip_ahead_stats()
    }

    /// Hand tasks cancelled at shutdown to `sink` (default: a
    /// [`LoggingCancellationSink`], which logs and drops them).
    ///
    /// While a sink is registered the pool keeps a JSON snapshot
    /// (`serde_json::Value`) of each running task's payload, so a running
    /// straggler cancelled at the drain deadline reaches the sink with its
    /// payload. That costs one serialization per started task.
    #[must_use]
    pub fn with_cancellation_sink(self, sink: Arc<dyn CancellationSink<P>>) -> Self {
        *self.inner.sink.write() = Some(sink);
        self
    }

    /// Bound each sink call by `config.delta` (default 5 s). The other fields
    /// of [`CancellationConfig`] do not apply to the pool.
    #[must_use]
    pub fn with_cancellation_config(self, config: CancellationConfig) -> Self {
        *self.inner.cancellation.write() = config;
        self
    }

    /// Register a lifecycle hook (cr-31); see the module docs for the events.
    /// Hooks run on the bus's own thread, so a slow or panicking hook never
    /// stalls the pool.
    #[must_use]
    pub fn with_hook(self, hook: Arc<dyn Hook>) -> Self {
        self.inner.hooks.register(hook);
        self
    }

    /// The pool's hook bus: register more hooks at any time, read its
    /// counters, or [`flush`](HookBus::flush) it.
    #[must_use]
    pub fn hooks(&self) -> &HookBus {
        &self.inner.hooks
    }

    /// Resource units currently held by running tasks.
    #[must_use]
    pub fn active_units(&self) -> u32 {
        self.inner.dispatcher.units_in_use()
    }

    /// Units of dimension `kind` held by running tasks, or `None` when the
    /// pool does not limit `kind` (cr-23; [`active_units`](Self::active_units)
    /// is the total over every kind).
    #[must_use]
    pub fn active_units_of(&self, kind: ResourceKind) -> Option<u32> {
        self.inner.dispatcher.units_in_use_of(kind)
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

    /// Stop accepting new submissions, and nothing else.
    ///
    /// Later calls to [`submit`](Self::submit) return
    /// [`SchedulerError::PoolShutdown`]. Every accepted task still runs:
    /// running tasks finish and queued tasks keep starting as capacity frees,
    /// with no deadline. To bound that and capture what is left, use
    /// [`shutdown_with`](Self::shutdown_with).
    pub fn shutdown(&self) {
        // SeqCst: pairs with the `in_flight` increment in `submit` (see
        // `crate::ledger`).
        self.inner.shutdown.store(true, Ordering::SeqCst);
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
    ///   pool's total capacity or a per-kind limit, so it could never start;
    /// - [`SchedulerError::QueueFull`] when the task must wait and the queue is
    ///   at its depth limit (checked atomically with the enqueue);
    /// - the queue backend's error when enqueueing fails.
    pub async fn submit(&self, task: ScheduledTask<P>) -> Result<TaskStatus, SchedulerError> {
        let inner = &self.inner;
        let events = inner.events();
        let task_id = task.meta.id;
        events.submitted(&task.meta);
        // Count the task before reading the flag; SeqCst on both sides (see
        // `crate::ledger`): either this submission is refused or a concurrent
        // `shutdown_with` counts it and waits for it.
        inner.ledger.accept();
        if inner.shutdown.load(Ordering::SeqCst) {
            inner.ledger.settle(1);
            let error = SchedulerError::PoolShutdown {
                pool: inner.dispatcher.pool_name().to_owned(),
            };
            events.rejected(task_id, &error);
            return Err(error);
        }
        if inner.is_expired(&task.meta) {
            inner.ledger.settle(1);
            tracing::warn!(task_id, "task expired before enqueue");
            let error = SchedulerError::DeadlineExpired { task_id };
            events.rejected(task_id, &error);
            return Err(error);
        }
        inner.ensure_release_hook();
        let now_ms = inner.now_ms();
        let meta = task.meta.clone();
        let admission = inner
            .dispatcher
            .submit_with(task, now_ms, |depth| events.queued(task_id, depth))
            .inspect_err(|error| {
                inner.ledger.settle(1);
                tracing::warn!(task_id, %error, "task rejected at admission");
                events.rejected(task_id, error);
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
        // A `shutdown_with` that took the parked queue before this task was
        // parked cannot see it. The park happened inside the dispatch lock, so
        // if the flag reads false here `take_parked` (which takes the lock
        // after the flag is stored) found the task; if it reads true, capture
        // whatever is parked now.
        if inner.parked_closed.load(Ordering::SeqCst) {
            inner.capture_parked().await;
        }
        Ok(status)
    }

    /// Shut the pool down under `policy` and report what happened to every
    /// accepted task.
    ///
    /// Submissions are refused at once. Until the policy's drain deadline,
    /// running tasks finish and queued tasks keep starting as capacity frees;
    /// the call returns as soon as every accepted task is settled. At the
    /// deadline, queued tasks that never started are cancelled (reason
    /// [`CancelReason::QueueShutdown`]); under `DrainThenCancel` and
    /// `CancelImmediately` running tasks are cancelled too, at their next
    /// await point ([`CancelReason::DrainTimeout`]). Each cancelled task's
    /// mailbox receives [`TaskError::Cancelled`] and the task is offered to
    /// the cancellation sink, each call bounded by
    /// [`CancellationConfig::delta`]. The call then waits at most `delta`
    /// plus one second for cancelled stragglers to settle.
    ///
    /// The report's `unsettled` counts tasks still running when it returns (a
    /// `Drain` straggler, or an executor that blocks without yielding); they
    /// are not dropped and deliver their outcome later. Calling it again runs
    /// the same steps for whatever is left.
    pub async fn shutdown_with(&self, policy: ShutdownPolicy) -> ShutdownReport {
        let inner = &self.inner;
        let since = inner.ledger.tally();
        self.shutdown();
        tracing::info!(?policy, "shutting down resource pool");
        let deadline = policy.drain_deadline();
        let drained = inner
            .ledger
            .wait_until(deadline, |ledger| ledger.in_flight() == 0)
            .await;
        if !drained {
            // SeqCst: pairs with the load in `submit` (see there).
            inner.parked_closed.store(true, Ordering::SeqCst);
            inner.capture_parked().await;
            if policy.cancels_stragglers() {
                inner.cancel.cancel();
                let bound = inner.cancellation.read().delta + SETTLE_MARGIN;
                inner
                    .ledger
                    .wait_until(bound, |ledger| ledger.in_flight() == 0)
                    .await;
            }
        }
        let report = inner.ledger.report(since);
        tracing::info!(?report, "resource pool shut down");
        report
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
        let removed_count = u64::try_from(removed).unwrap_or(u64::MAX);
        inner.ledger.finish(removed_count);
        inner.ledger.settle(removed_count);
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

    fn events(&self) -> Events<'_> {
        Events {
            bus: &self.hooks,
            pool: self.dispatcher.pool_name(),
        }
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

    /// Install the dispatcher's release hook (once): a permit dropped other
    /// than through `Dispatcher::complete` — a task future the spawner
    /// discarded without running it — schedules a poke through the spawner,
    /// so the units it frees reach parked work. Every other path returns its
    /// permit through `complete`, or pokes after catching a panic.
    ///
    /// Installed at the first submission rather than in `new`: the hook holds
    /// a `Weak` to this state, and the builder methods need the state
    /// unshared.
    fn ensure_release_hook(self: &Arc<Self>) {
        if self.dispatcher.has_release_hook() {
            return;
        }
        self.dispatcher.install_release_hook(|| {
            let weak = Arc::downgrade(self);
            move || {
                if let Some(inner) = weak.upgrade() {
                    inner.poke_later();
                }
            }
        });
    }

    /// Spawn a drain of the parked queue and start what it admits (the
    /// release hook's job; it must not drain on the dropping thread, which
    /// may hold this pool's locks).
    fn poke_later(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        self.spawner.spawn(async move {
            let woken = inner.dispatcher.poke(inner.now_ms());
            inner.launch(woken, None);
        });
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
    /// twice. The completion starts the tasks it wakes before it calls the
    /// `Mailbox` or `AuditSink`, so a panic there loses none of them.
    async fn supervise(self: Arc<Self>, task: ScheduledTask<P>, permit: Permit) {
        // Settles the task however `run` ends, unwinding included.
        let _settle = SettleOnDrop(&self.ledger);
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
                let error = TaskError::from(panicked);
                self.events().failed(task_id, &error);
                if let Some(key) = mailbox_key.as_ref() {
                    let status = TaskStatus::Failed(error);
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
    ///
    /// A task started after the shutdown's cancel phase began is captured
    /// without running. A running task is raced against the pool's cancel
    /// token: when it fires, the executor future is dropped and the task is
    /// captured with its payload restored from the snapshot.
    async fn run(self: Arc<Self>, task: ScheduledTask<P>, permit: Permit, reported: &AtomicBool) {
        let ScheduledTask { meta, payload } = task;
        let task_id = meta.id;
        if self.cancel.is_cancelled() {
            reported.store(true, Ordering::Relaxed);
            let woken = self.dispatcher.complete(permit, self.now_ms());
            self.launch(woken, None);
            self.capture(meta, Some(payload), CancelReason::QueueShutdown)
                .await;
            return;
        }
        let mailbox_key = meta.mailbox.clone();
        let budget = self.budget(&meta);
        let snapshot = self.snapshot(task_id, &payload);
        let kept_meta = meta.clone();
        tracing::debug!(task_id, ?budget, "executing task");
        let events = self.events();
        events.started(task_id, 0);
        let started = events.clock();

        // On elapse (or cancellation) the executor future is dropped, which
        // cancels it at its next await point; an executor owning a
        // `tokio::process::Child` built with `kill_on_drop(true)` has the
        // child reaped by that drop.
        let executor = &self.executor;
        let cancel = &self.cancel;
        let contained = contain(async move {
            tokio::select! {
                biased;
                () = cancel.cancelled() => None,
                ran = tokio::time::timeout(budget, executor.execute(payload, meta)) => Some(ran),
            }
        })
        .await;
        let outcome = match contained {
            Ok(None) => {
                tracing::warn!(task_id, "running task cancelled at the drain deadline");
                reported.store(true, Ordering::Relaxed);
                let woken = self.dispatcher.complete(permit, self.now_ms());
                self.launch(woken, None);
                let payload = snapshot.and_then(|value| Self::restore(task_id, value));
                self.capture(kept_meta, payload, CancelReason::DrainTimeout)
                    .await;
                return;
            }
            Ok(Some(Ok(outcome))) => outcome,
            Ok(Some(Err(_elapsed))) => Err(TaskError::Timeout { budget }),
            Err(panicked) => Err(TaskError::from(panicked)),
        };

        reported.store(true, Ordering::Relaxed);
        events.finished(task_id, &outcome, started);
        self.finish(task_id, permit, mailbox_key.as_ref(), outcome);
    }

    /// Release a finished task's capacity, deliver its outcome, audit it and
    /// start whatever queued work now fits.
    ///
    /// The permit is returned, and the woken tasks taken off the queue and
    /// started, before the outcome is delivered: a caller that reads the
    /// outcome from the mailbox already sees the units released, and a panic
    /// in the user's `Mailbox` or `AuditSink` cannot lose the woken tasks.
    fn finish(
        self: &Arc<Self>,
        task_id: TaskId,
        permit: Permit,
        mailbox_key: Option<&MailboxKey>,
        outcome: Result<T, TaskError>,
    ) {
        let woken = self.dispatcher.complete(permit, self.now_ms());
        self.launch(woken, None);
        let counter = if outcome.is_ok() {
            &self.completed
        } else {
            &self.failed
        };
        // Relaxed: see `ResourcePool::completed_tasks`.
        counter.fetch_add(1, Ordering::Relaxed);
        self.ledger.finish(1);
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
    }

    /// Record a queued task that reached the head after its deadline.
    fn expire(&self, meta: &TaskMetadata) {
        self.events().expired(meta.id);
        self.failed.fetch_add(1, Ordering::Relaxed);
        self.ledger.finish(1);
        self.ledger.settle(1);
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

    /// Take every parked task out of the queue and capture it as never
    /// started; each is settled once its sink call ends.
    async fn capture_parked(&self) {
        for task in self.dispatcher.take_parked() {
            let ScheduledTask { meta, payload } = task;
            self.capture(meta, Some(payload), CancelReason::QueueShutdown)
                .await;
            self.ledger.settle(1);
        }
    }

    /// Record a cancelled task: deliver [`TaskError::Cancelled`] to its
    /// mailbox, audit it, and offer it to the sink under `delta`. Settling is
    /// the caller's (a running task settles when `supervise` returns).
    async fn capture(&self, meta: TaskMetadata, payload: Option<P>, reason: CancelReason) {
        self.events().cancelled(&meta, &reason, 0);
        let task_id = meta.id;
        tracing::warn!(task_id, ?reason, "task cancelled at shutdown");
        if let Some(key) = meta.mailbox.as_ref() {
            self.deliver(task_id, key, TaskStatus::Failed(shutdown_cancelled()), None);
        }
        let tenant = meta
            .mailbox
            .as_ref()
            .map_or("unknown", |key| key.tenant.as_str());
        self.record_event(task_id, tenant, "cancel");
        let sink = self.sink.read().clone().unwrap_or_else(|| {
            Arc::new(LoggingCancellationSink::new(self.dispatcher.pool_name()))
                as Arc<dyn CancellationSink<P>>
        });
        let delta = self.cancellation.read().delta;
        let cancelled = CancelledTask {
            payload,
            meta,
            reason,
            attempt: 0,
            cancelled_at_ms: u64::try_from(self.now_ms()).unwrap_or(u64::MAX),
        };
        self.ledger.begin_capture();
        let answer = offer(&sink, cancelled, delta).await;
        self.ledger.end_capture(task_id, answer.as_ref());
    }

    /// A JSON copy of a running task's payload, kept only while a sink is
    /// registered (only the sink can use it).
    fn snapshot(&self, task_id: TaskId, payload: &P) -> Option<serde_json::Value> {
        if self.sink.read().is_none() {
            return None;
        }
        serde_json::to_value(payload)
            .inspect_err(|error| {
                tracing::warn!(task_id, %error, "payload snapshot failed; a cancelled run reaches the sink without it");
            })
            .ok()
    }

    /// Rebuild a payload from its snapshot.
    fn restore(task_id: TaskId, snapshot: serde_json::Value) -> Option<P> {
        serde_json::from_value(snapshot)
            .inspect_err(|error| {
                tracing::warn!(task_id, %error, "payload snapshot did not deserialize; the sink gets the task without it");
            })
            .ok()
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
