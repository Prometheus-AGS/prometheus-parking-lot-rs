//! The WASM `WorkerPool`: the shared dispatcher driving local async tasks.
//!
//! # Design
//!
//! - **One dispatcher**: admission goes through the core
//!   [`Dispatcher`](prometheus_parking_lot_core::dispatch::Dispatcher), the
//!   same one the native pool uses. It enforces `max_units` (audit C1: the 0.2
//!   WASM pool ignored it), serves parked tasks by priority then submission
//!   order, and bounds admitted tasks by `worker_count`, so `max_queue_depth`
//!   bounds exactly the tasks waiting. A parked task that fits may skip ahead
//!   of a head that does not, at most `WorkerPoolConfig::max_head_skips`
//!   times per head (cr-20).
//! - **Local tasks**: an admitted task runs on
//!   `wasm_bindgen_futures::spawn_local`. There is no tokio runtime and no
//!   thread; the browser event loop is the scheduler.
//! - **Bounded execution**: every execution runs under a budget (the task's
//!   deadline or the pool's default timeout, whichever is nearer), measured by
//!   a JS `setTimeout`. A task still running at its budget is dropped at its
//!   next await point, its units are released and its result is
//!   [`TaskError::Timeout`].
//! - **Lifecycle events** (cr-53): hooks registered with
//!   [`WorkerPool::with_hook`] see, per task (keyed by
//!   [`TaskMetadata::id`]), the same sequence as the native pools:
//!   `Submitted` first in `submit`; `Rejected` when `submit` refuses it;
//!   `Queued` under the dispatch lock when it parks; `Started` when its
//!   executor is about to run (`worker_id` is always 0: there are no
//!   workers); then one terminal event: `Completed`/`Failed` when the
//!   executor returns, is cut off at its budget or ends (before its units are
//!   returned), `Expired` when it reached the head after its deadline, or
//!   `Cancelled` when a shutdown captured it. Hooks run inline.
//! - **No polling**: results use a oneshot per waiting retriever.
//! - **Clean shutdown**: parked tasks are failed with [`TaskError::Cancelled`]
//!   rather than dropped silently.
//!
//! # Panics abort the module
//!
//! `wasm32-unknown-unknown` is built with `panic=abort` on stable Rust, so an
//! executor panic cannot be caught: it traps and the whole module is dead.
//! The panic containment of the native pools (`TaskError::ExecutorPanicked`)
//! does not exist here. Executors must return errors, not panic.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::RwLock;
use tracing::{debug, info, warn};
use wasm_bindgen_futures::spawn_local;

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::dispatch::{
    Dispatched, Dispatcher, Grant, PoolCounters, SkipAheadStats, WorkerTask, generate_mailbox_key,
    mailbox_key_to_string,
};
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{
    Clock, Hook, HookBus, MailboxKey, PoolStats, SchedulerError, TaskError, TaskMetadata,
    WorkerExecutor,
};

use crate::clock::WasmClock;
use crate::events::Events;
use crate::results::{Claim, Results};
use crate::time::{Elapsed, timeout};

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

/// The error for a task that will not run because the pool shut down.
fn shutdown_cancelled() -> TaskError {
    TaskError::Cancelled {
        reason: "worker pool shut down".into(),
    }
}

/// State shared by the pool handle and its running tasks.
struct Shared<P, R, E> {
    /// Pool configuration.
    config: WorkerPoolConfig,
    /// Admission, the parked queue and the semaphore.
    ///
    /// No release hook (`Dispatcher::install_release_hook`): `run` returns
    /// every permit through `complete`. `spawn_local` never discards a task
    /// future, and a panic aborts the module, so no permit is dropped any
    /// other way.
    dispatcher: Dispatcher<WorkerTask<P>>,
    /// Result slots with oneshot notification.
    results: Results<TaskOutcome<R>>,
    /// Submitted/completed/failed counters (running and parked come from the
    /// dispatcher).
    counters: PoolCounters,
    /// Time source for deadlines and budgets.
    clock: RwLock<Arc<dyn Clock>>,
    /// Shutdown flag.
    shutdown: AtomicBool,
    /// Lifecycle-event hooks (cr-53); delivery is inline on this target.
    hooks: HookBus,
    /// The executor; each task runs on a clone.
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

    fn events(&self) -> Events<'_> {
        Events {
            bus: &self.hooks,
            pool: POOL_NAME,
            clock: &self.clock,
        }
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
    /// unbounded), so the wedged-executor test must fail.
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
        self.results.store(&mailbox_key_to_string(key), outcome);
    }

    /// Start every admitted task on the local executor and fail every task
    /// found expired at the head of the queue.
    fn launch(self: &Arc<Self>, dispatched: Dispatched<WorkerTask<P>>) {
        let Dispatched {
            started,
            expired,
            rejected,
        } = dispatched;
        // Normal submissions are validated; settle invalid pre-populated items.
        for (task, error) in rejected {
            self.events().rejected(task.meta.id, &error);
            self.finish(
                &task.mailbox_key,
                Err(TaskError::Cancelled {
                    reason: error.to_string(),
                }),
            );
        }
        for task in expired {
            warn!(
                task_id = task.meta.id,
                "queued task expired before it could start"
            );
            self.events().expired(task.meta.id);
            self.finish(&task.mailbox_key, Err(TaskError::DeadlineExpired));
        }
        for admitted in started {
            spawn_local(Arc::clone(self).run(admitted));
        }
    }

    /// Fail every parked task (shutdown): nothing parked is dropped silently.
    fn cancel_parked(&self) {
        for task in self.dispatcher.take_parked() {
            self.events().cancelled(&task.meta);
            self.finish(&task.mailbox_key, Err(shutdown_cancelled()));
        }
    }

    /// Run one admitted task under its budget, then return its permit and
    /// start whatever that admits.
    async fn run(self: Arc<Self>, admitted: Grant<WorkerTask<P>>) {
        let Grant { item: task, permit } = admitted;
        let WorkerTask {
            payload,
            meta,
            mailbox_key,
        } = task;
        let task_id = meta.id;

        let events = self.events();
        let outcome = if self.shutdown.load(Ordering::Acquire) {
            // Acquire pairs with the Release in `shutdown()`: the pool is
            // shutting down, so the task is failed rather than started.
            events.cancelled(&meta);
            Err(shutdown_cancelled())
        } else {
            let budget = self.budget(&meta);
            debug!(task_id, ?budget, "WASM worker executing task");
            events.started(task_id);
            let started = events.timer();
            // On elapse the executor future is dropped, which cancels it at
            // its next await point.
            let outcome = timeout(budget, self.executor.execute(payload, meta))
                .await
                .unwrap_or_else(|Elapsed| {
                    warn!(
                        task_id,
                        ?budget,
                        "task exceeded its budget and was cancelled"
                    );
                    Err(TaskError::Timeout { budget })
                });
            events.finished(task_id, &outcome, started);
            outcome
        };

        // Return the units before publishing the outcome, so a caller that
        // has retrieved the result already sees them released.
        let woken = self.dispatcher.complete(permit, self.now_ms());
        self.finish(&mailbox_key, outcome);
        self.launch(woken);
    }
}

/// Worker pool for WASM: the shared dispatcher driving `spawn_local` tasks.
///
/// Same admission and result semantics as the native pool in
/// `prometheus-parking-lot-runtime-tokio`, without the blocking APIs (WASM
/// cannot block) and without panic containment (see the module docs: panics
/// abort the module).
///
/// # Design
///
/// - **Capacity**: a task runs only while the units of all running tasks,
///   including it, fit in `max_units` and in every per-kind limit of
///   `resource_limits` (cr-23), and at most `worker_count` tasks run.
///   Others wait in a priority queue (at most `max_queue_depth`), highest
///   priority first, FIFO within a priority.
/// - **Budgets**: each execution is bounded by the task's deadline or
///   `default_timeout_ms`, whichever is nearer.
/// - **Executor futures must be `Send`** (the `WorkerExecutor` contract), so
///   a raw JS timer future cannot be awaited inside one; use
///   [`sleep`](crate::sleep).
pub struct WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// State shared with the running tasks.
    shared: Arc<Shared<P, R, E>>,
    /// Task ID counter (lock-free).
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
    /// Time is read from [`WasmClock`] until [`with_clock`](Self::with_clock)
    /// replaces it.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::InvalidConfig`] if the configuration is invalid.
    pub fn new(config: WorkerPoolConfig, executor: E) -> Result<Self, SchedulerError> {
        config.validate().map_err(SchedulerError::invalid_config)?;
        let worker_permits = u32::try_from(config.worker_count).unwrap_or(u32::MAX);

        info!(
            worker_count = config.worker_count,
            max_units = config.max_units,
            max_queue_depth = config.max_queue_depth,
            "WorkerPool (WASM) initialized on the shared dispatcher"
        );

        let shared = Arc::new(Shared {
            dispatcher: Dispatcher::in_memory(
                POOL_NAME,
                UnitSemaphore::with_max_permits(config.max_units, worker_permits)
                    .with_resource_limits(config.resource_limits.clone()),
                config.max_queue_depth,
            )
            .with_max_head_skips(config.max_head_skips),
            results: Results::new(),
            counters: PoolCounters::default(),
            clock: RwLock::new(Arc::new(WasmClock::new())),
            shutdown: AtomicBool::new(false),
            hooks: HookBus::new(),
            executor,
            config,
        });
        Ok(Self {
            shared,
            task_id_counter: AtomicU64::new(0),
        })
    }

    /// Read time from `clock` instead of [`WasmClock`]: deadline checks at
    /// submission and at the head of the queue, and execution budgets.
    ///
    /// Budgets are still measured by the JS timer; the clock decides how much
    /// of a task's deadline remains when it starts.
    #[must_use]
    pub fn with_clock(self, clock: impl Clock + 'static) -> Self {
        *self.shared.clock.write() = Arc::new(clock);
        self
    }

    /// Register a lifecycle hook (cr-53); see the module docs for the events.
    /// There are no threads on this target: hooks run inline, on the call
    /// that causes the transition, so they must be quick and must not call
    /// back into the pool.
    #[must_use]
    pub fn with_hook(self, hook: Arc<dyn Hook>) -> Self {
        self.shared.hooks.register(hook);
        self
    }

    /// The pool's hook bus: register more hooks at any time or read its
    /// counters.
    #[must_use]
    pub fn hooks(&self) -> &HookBus {
        &self.shared.hooks
    }

    /// Skip-ahead counters (cr-20): tasks started in place of a head that
    /// did not fit, and heads for which the starvation guard engaged.
    #[must_use]
    pub fn skip_ahead_stats(&self) -> SkipAheadStats {
        self.shared.dispatcher.skip_ahead_stats()
    }

    /// Submit a task asynchronously. Admission never waits; this is
    /// [`submit`](Self::submit) behind an async signature.
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
    /// units and a worker slot is free; otherwise it waits in the priority
    /// queue. It runs on the browser event loop, so it makes progress only
    /// while the caller yields.
    ///
    /// # Returns
    ///
    /// A `MailboxKey` to retrieve the result with.
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
        let events = shared.events();
        let id = meta.id;
        events.submitted(&meta);
        // Acquire pairs with the Release in `shutdown()`.
        if shared.shutdown.load(Ordering::Acquire) {
            let error = pool_shutdown();
            events.rejected(id, &error);
            return Err(error);
        }
        let now_ms = shared.now_ms();
        if meta.deadline_ms.is_some_and(|deadline| now_ms > deadline) {
            let error = SchedulerError::DeadlineExpired { task_id: id };
            events.rejected(id, &error);
            return Err(error);
        }

        // Relaxed: the counter only needs unique values.
        let task_id = self.task_id_counter.fetch_add(1, Ordering::Relaxed);
        let mailbox_key = generate_mailbox_key(task_id);
        let slot = mailbox_key_to_string(&mailbox_key);
        shared.results.create(&slot);

        let task = WorkerTask {
            payload,
            meta,
            mailbox_key: mailbox_key.clone(),
        };
        let admission = match shared
            .dispatcher
            .submit_with(task, now_ms, |depth| events.queued(id, depth))
        {
            Ok(admission) => admission,
            Err(error) => {
                shared.results.remove(&slot);
                warn!(%error, "worker pool rejected task");
                events.rejected(id, &error);
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
            "task submitted to WASM worker pool"
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
    /// Waits for the result or the timeout (a JS timer). No polling: the
    /// worker that finishes the task wakes the waiter.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::Task`] carrying the [`TaskError`] when the executor
    ///   failed, ran past its budget ([`TaskError::Timeout`]), expired while
    ///   queued or was cancelled at shutdown
    /// - [`SchedulerError::Timeout`] if the result is not available within the
    ///   timeout (the slot is then discarded)
    /// - [`SchedulerError::ResultNotFound`] if the mailbox key is invalid
    pub async fn retrieve_async(
        &self,
        key: &MailboxKey,
        timeout_after: Duration,
    ) -> Result<R, SchedulerError> {
        let results = &self.shared.results;
        let slot = mailbox_key_to_string(key);
        let notified = match results.claim(&slot) {
            Claim::Ready(outcome) => return outcome.map_err(SchedulerError::Task),
            Claim::Missing => return Err(result_not_found(key)),
            Claim::Wait(notified) => notified,
        };

        let waited = timeout(timeout_after, notified).await;
        // The slot goes on every path: a retrieved or timed-out result is gone.
        let outcome = results.remove(&slot);
        match waited {
            Ok(_) => outcome
                .ok_or_else(|| {
                    SchedulerError::internal("result notification ended without an outcome")
                })?
                .map_err(SchedulerError::Task),
            Err(_elapsed) => Err(SchedulerError::Timeout { task_id: None }),
        }
    }

    /// Get current pool statistics.
    ///
    /// `failed_tasks` counts executor errors, timeouts, expired deadlines and
    /// shutdown cancellations.
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

    /// Shut down the pool.
    ///
    /// New submissions are refused, tasks still waiting in the queue are
    /// failed with [`TaskError::Cancelled`], admitted tasks that have not
    /// started are failed the same way, and running tasks finish (within
    /// their budgets).
    pub fn shutdown(&self) {
        let shared = &self.shared;
        // AcqRel: Release publishes the flag to the Acquire loads in
        // `submit`/`run`; Acquire makes a second caller see the first.
        if shared.shutdown.swap(true, Ordering::AcqRel) {
            return; // Already shut down
        }
        info!("Shutting down WASM worker pool");
        shared.cancel_parked();
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
