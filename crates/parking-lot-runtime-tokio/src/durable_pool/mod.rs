//! Durable mode of the resource pool (cr-49): accepted work survives a crash
//! of the pool process.
//!
//! A [`DurableResourcePool`] runs tasks exactly like
//! [`ResourcePool`](crate::ResourcePool) — the same unit-weighted capacity,
//! priority-then-FIFO admission, execution budget and panic containment —
//! but keeps every accepted task in a durable [`LeaseQueue`] and every outcome
//! in a durable [`ResultMailbox`].
//!
//! # The durability sequence
//!
//! 1. **Persist before accept.** [`submit`](DurableResourcePool::submit)
//!    returns `Ok` only after [`LeaseQueue::enqueue`] has resolved, which the
//!    contract defines as "stored as durably as the backend can".
//! 2. **Lease.** One dispatcher task per pool leases the queue's head
//!    (`lease(1, lease_for)`; the queue orders by priority, then enqueue
//!    order) whenever the pool has a free unit and nothing leased is still
//!    waiting for capacity. The leased task goes through the core
//!    [`Dispatcher`]: it starts when its units fit, otherwise it waits at the
//!    head and blocks further leasing until completions free its units.
//! 3. **Heartbeat.** From the moment it is leased until its lease is resolved,
//!    every task has a heartbeat that calls [`LeaseQueue::extend`] every
//!    [`heartbeat_interval`](DurableOptions::heartbeat_interval) (a third of
//!    the lease by default), so a task that runs, waits for capacity or
//!    retries its delivery for longer than the lease is never redelivered
//!    while this process is alive.
//! 4. **Execute** under the task's budget, inside the cr-21 panic containment.
//!    The task's units are returned as soon as the executor finishes.
//! 5. **Deliver** the outcome to the [`ResultMailbox`] with a stable
//!    idempotency key (see below), retrying a failed delivery while the
//!    heartbeat keeps the lease.
//! 6. **Stop the heartbeat, then resolve the lease**: ack a success,
//!    dead-letter a failure (see the table). The lease is resolved only after
//!    the delivery is stored, so a crash at any earlier point leaves the task
//!    leased, its lease expires and the queue redelivers it.
//!
//! | outcome | mailbox | lease |
//! |---|---|---|
//! | the executor returned a value | `Completed` with the value | ack |
//! | executor error, panic, budget exceeded | `Failed(error)` | dead-letter |
//! | deadline passed before the task started | `Expired` | dead-letter |
//! | cost exceeds this pool's capacity, attempt below the oversize limit (another consumer may fit it) | nothing | nack (requeue after one lease duration) |
//! | cost exceeds this pool's capacity at the oversize attempt limit | `Dropped(reason)` | dead-letter |
//! | delivery still failing after retries | nothing | nack (requeue) |
//! | pool shut down before the task started | nothing | nack (requeue) |
//! | cancelled by `shutdown_with` while running | nothing | nack (requeue) |
//! | process killed at any point | nothing new | expires, redelivered |
//!
//! Failures are final, as in the in-memory pool (no automatic retry); they are
//! dead-lettered rather than acked so operators keep a record of them in the
//! queue. A task that kills its process is redelivered on every restart; cap
//! that with the backend's own attempt limit (for example
//! `PgLeaseQueue::with_max_attempts`), which dead-letters it without a
//! mailbox delivery.
//!
//! A task too large for this pool (it arrived through
//! [`LeaseQueue::enqueue`] directly, or from a larger pool's `submit`) is
//! handed back, one lease duration later, for a consumer that can fit it. The
//! hand-back is bounded by
//! [`DurableOptions::with_oversize_attempt_limit`] (default 5): a pool that
//! leases the task at that attempt or later, and cannot run it, settles it
//! instead — `Dropped` in the mailbox, a `Rejected` event
//! (`CapacityExceeded`) and a dead letter — so a client waiting on the task's
//! mailbox key gets a terminal status within about `limit - 1` lease
//! durations. A consumer that can run the task runs it at any attempt. With
//! a backend attempt limit, keep the oversize limit at or below it, or the
//! backend dead-letters the task first, without a mailbox delivery.
//!
//! `max_units` bounds the units of running tasks; a zero-cost task takes no
//! unit, so the number of running tasks is bounded separately by
//! [`DurableOptions::with_max_running`] (default: `max_units`).
//!
//! # Exactly one consumable result
//!
//! Every delivery carries an idempotency key, and the mailbox keeps the first
//! write for a key (ADR 0003), including after it was consumed:
//!
//! - the task's own [`idempotency_key`](TaskMetadata::idempotency_key) when
//!   the caller set one, so a client that resubmits a finished task gets no
//!   second result either;
//! - otherwise `"<queue identity>:entry:<queue entry id>"`. The entry id is
//!   assigned once at enqueue and travels with every redelivery of the entry
//!   (it is the [`LeaseToken::entry`]); [`LeaseQueue::identity`] names the
//!   queue the same way in every process. So every execution of one accepted
//!   task delivers under one key, whichever pool runs it.
//!
//! A redelivered task that already delivered its result therefore runs again
//! but stores nothing new: the consumer sees exactly one result. The key does
//! not involve the pool's [name](DurableOptions::with_name): any number of
//! pools, named alike or differently, can consume one queue, and pools on
//! different queues can deliver into one mailbox, without a duplicate or a
//! lost result. One limit: a queue dropped and recreated under the same
//! identity can reuse entry ids (see [`LeaseQueue::identity`]); recreate its
//! mailbox with it, or set task idempotency keys.
//!
//! # Where duplicates still execute
//!
//! Delivery is at-least-once, so the *executor* can run more than once for one
//! accepted task: when the process dies after the executor started and before
//! the ack; when a delivery or ack fails past its retries; and when this
//! process stalls for longer than the lease (the heartbeat then gets
//! [`SchedulerError::LeaseLost`]). On a lost lease the running execution is
//! deliberately **not** cancelled: cancelling mid-flight could leave external
//! side effects half done while not preventing the other execution, and the
//! idempotent delivery already makes the extra result harmless. The fenced ack
//! then fails with `LeaseLost` and the entry's current holder resolves it.
//! Executors with external side effects must be idempotent.
//!
//! # Shutdown (cr-29)
//!
//! [`shutdown_with`](DurableResourcePool::shutdown_with) stops leasing at
//! once under every [`ShutdownPolicy`]: tasks never leased stay in the queue,
//! and a leased task still waiting for capacity is handed back
//! (`nack(Requeue)`). Running tasks finish within the drain deadline; under
//! `DrainThenCancel` and `CancelImmediately` the ones still running then are
//! cancelled (the executor future is dropped), their heartbeat stops and they
//! are handed back too, so the next pool started on the queue runs them. The
//! queue is the capture: the durable pool does not use a `CancellationSink`.
//! A cancelled execution may have had side effects before it was dropped, so
//! a requeued task can run twice (see the section above).
//!
//! # Lifecycle events (cr-31)
//!
//! The pool emits on [`DurableOptions::hooks`], keyed by
//! [`TaskMetadata::id`].
//!
//! **The durable contract is one terminal event per run**, not per task: a
//! task can run several times (in this process or others), and each run ends
//! with exactly one terminal event. Every run but the last ends in
//! `Requeued`; nothing follows a terminal event other than `Requeued`, after
//! which the next run's events may follow. So within one process a task's
//! events are `Submitted`, then one or more runs of the form
//! `[Queued] [Started] terminal`, each run but the last ending in `Requeued`.
//!
//! - `Submitted` first in `submit`. `Rejected` when `submit` refuses the task
//!   for certain (shutdown, expired deadline, a cost over this pool's
//!   capacity, a full queue, a backend failure before anything was written,
//!   [`SchedulerError::NotStored`]): nothing is stored. When the enqueue
//!   fails with an unknown outcome ([`SchedulerError::Backend`]), no terminal
//!   event is emitted: the task may have been stored and run, and that run
//!   emits its own events.
//! - Per run of a leased task: `Queued` when it waits for capacity (under the
//!   dispatch lock), `Started` just before the executor runs, then the run's
//!   terminal event, emitted when the lease is about to be resolved:
//!   `Completed` or `Failed` once the outcome is stored in the mailbox (or at
//!   once without a mailbox key), `Expired` for a deadline that passed before
//!   it started, `Rejected` (`CapacityExceeded`) for a task too large for
//!   this pool at the oversize attempt limit, and `Requeued` whenever the
//!   task goes back to the queue (shutdown, a cost too large for this pool
//!   below that limit, a delivery that keeps failing, or a panic outside the
//!   executor, which leaves the lease to expire).
//!
//! A run redelivered from the queue starts at `Queued`/`Started`; its
//! `Submitted` came from the process that accepted it. A redelivered run
//! reports its own outcome: after a crash between storing a `Failed` result
//! and dead-lettering it, the next run can emit `Completed` while the stored
//! result stays `Failed` (first write wins).
//!
//! # Ordering difference from the in-memory pool
//!
//! At most one leased task waits for capacity at a time. A task enqueued at a
//! higher priority after that head was leased waits behind it; in the
//! in-memory pool it would overtake the parked head.
//!
//! # No skip-ahead yet (cr-20)
//!
//! The in-memory pools let a parked task that fits start ahead of a head that
//! does not (bounded by a starvation guard). Here the one leased head is the
//! only parked task, so the dispatcher never has a candidate behind it and
//! dispatch stays strictly head-of-line: a durable pool does not read
//! `max_head_skips`. Supporting it means leasing a window of tasks behind a
//! blocked head, each holding a lease and a heartbeat while it waits, and
//! handing back the ones that never fit; that widens the set of tasks a crash
//! leaves leased (and so redelivered late) and the lease churn on the backend,
//! and it needs the lease-window crash tests before it can be claimed. It is
//! left as follow-up work rather than shipped untested.

mod options;

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use prometheus_parking_lot_core::dispatch::{Dispatched, Dispatcher, Grant, Parkable, Ticket};
use prometheus_parking_lot_core::semaphore::{Permit, UnitSemaphore, UnitVector};
use prometheus_parking_lot_core::{
    Delivery, LeaseQueue, LeaseToken, LeasedTask, MailboxKey, NackAction, PoolLimits,
    ResultMailbox, ScheduledTask, SchedulerError, Spawn, TaskError, TaskExecutor, TaskId,
    TaskMetadata, TaskPayload, TaskStatus, now_ms,
};
use tokio::sync::{Notify, oneshot};
use tokio_util::sync::CancellationToken;

use crate::contain::contain;
use crate::events::Events;
use crate::ledger::{Background, Held, Ledger};
use crate::shutdown::{ShutdownPolicy, ShutdownReport};

pub use self::options::DurableOptions;

/// Attempts at a mailbox delivery, or at an ack or nack, before giving up.
const ATTEMPTS: u32 = 5;
/// Delay after the first failed attempt; doubles after each further one.
const FIRST_BACKOFF: Duration = Duration::from_millis(100);
/// Whether an enqueue error means the task was certainly not stored (the
/// queue or the pool refused it), as opposed to an outcome the caller cannot
/// know (a backend or connection failure mid-commit).
const fn refused_for_sure(error: &SchedulerError) -> bool {
    matches!(
        error,
        SchedulerError::QueueFull { .. }
            | SchedulerError::NotStored { .. }
            | SchedulerError::CapacityExceeded { .. }
            | SchedulerError::DeadlineExpired { .. }
            | SchedulerError::PoolShutdown { .. }
            | SchedulerError::InvalidConfig { .. }
    )
}

/// How long `shutdown_with` waits, after cancelling running tasks, for them
/// to be handed back (a nack retries for about 1.5 s before giving up).
const CANCEL_GRACE: Duration = Duration::from_secs(5);

/// A resource pool whose accepted tasks and results live in a durable
/// [`LeaseQueue`] and [`ResultMailbox`]; see the module documentation for the
/// persist, lease, execute, deliver, ack sequence.
///
/// Start one with [`start`](Self::start) inside a tokio runtime. Dropping the
/// pool (or calling [`shutdown`](Self::shutdown)) stops leasing; tasks already
/// running finish, deliver and resolve their leases.
pub struct DurableResourcePool<P, T, Q, M, E, S> {
    inner: Arc<Inner<P, T, Q, M, E, S>>,
}

/// State shared by the pool handle, its dispatcher, heartbeats and tasks.
struct Inner<P, T, Q, M, E, S> {
    limits: PoolLimits,
    options: DurableOptions,
    /// Owns the semaphore and the (at most one) leased task waiting for
    /// capacity.
    dispatcher: Dispatcher<LeasedItem<P>>,
    queue: Q,
    mailbox: M,
    executor: E,
    spawner: S,
    /// Wakes the dispatcher: a submission, freed capacity, or shutdown.
    wake: Notify,
    shutdown: AtomicBool,
    /// Fired by `shutdown_with` at the drain deadline (policies that cancel
    /// stragglers): running tasks stop and are handed back to the queue.
    cancel: CancellationToken,
    /// Leased tasks held by this process (and the dispatcher while it runs),
    /// until resolved; how cancellations ended.
    ledger: Arc<Ledger>,
    completed: AtomicU64,
    failed: AtomicU64,
    _results: PhantomData<fn() -> T>,
}

/// A leased task inside this process, with the heartbeat that keeps its lease.
struct LeasedItem<P> {
    task: ScheduledTask<P>,
    token: LeaseToken,
    attempt: u32,
    heartbeat: Heartbeat,
    /// Counts the task in the ledger until it is resolved; dropped last.
    held: Held,
}

impl<P> Parkable for LeasedItem<P> {
    fn ticket(&self) -> Ticket {
        self.task.ticket()
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.task.set_sequence(sequence);
    }
}

/// Keeps a lease's heartbeat running; dropping it stops the heartbeat (the
/// heartbeat task sees the channel close).
struct Heartbeat {
    _stop: oneshot::Sender<()>,
}

/// What a finished (or never started) task leaves behind.
struct Settlement<T> {
    task_id: TaskId,
    mailbox: Option<MailboxKey>,
    idempotency_key: String,
    status: TaskStatus,
    payload: Option<T>,
    resolution: Resolution,
    /// The lease's attempt count, for a `Requeued` event.
    attempt: u32,
    /// Which terminal event the settlement emits once stored.
    terminal: Terminal,
}

/// The terminal lifecycle event of a settled task.
enum Terminal {
    /// It ran: `Completed`, or `Failed` for a `TaskStatus::Failed` status.
    /// Holds when it started, for the event's duration.
    Ran(Option<Instant>),
    /// Its deadline passed before it started.
    Expired,
    /// It is too large for this pool and reached the oversize attempt limit:
    /// `Rejected` with the capacity error.
    TooLarge(SchedulerError),
}

/// How a settled task's lease is resolved.
enum Resolution {
    /// The task completed: remove it.
    Ack,
    /// The task failed for good: keep it as a dead letter with this reason.
    DeadLetter(String),
}

impl<P, T, Q, M, E, S> DurableResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    Q: LeaseQueue<P> + 'static,
    M: ResultMailbox<T> + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Send + Sync + 'static,
{
    /// Start a durable pool: spawn its dispatcher through `spawner`, which
    /// begins leasing from `queue` at once, so work accepted before a restart
    /// (and not acked) is picked up as its leases expire.
    ///
    /// `limits.max_units` bounds the units of running tasks and
    /// `limits.default_timeout` the execution budget; `queue`'s own maximum
    /// depth bounds accepted work (`limits.max_queue_depth` is not used: no
    /// task waits in this process except the one leased head).
    ///
    /// # Errors
    ///
    /// [`SchedulerError::InvalidConfig`] when `options` has a zero lease, a
    /// heartbeat interval that is zero or not shorter than the lease, a zero
    /// poll interval, a zero running-task bound or an empty name.
    pub fn start(
        limits: PoolLimits,
        queue: Q,
        mailbox: M,
        executor: E,
        spawner: S,
        options: DurableOptions,
    ) -> Result<Self, SchedulerError> {
        options.validate()?;
        let dispatcher = Dispatcher::in_memory(
            options.name().to_owned(),
            UnitSemaphore::with_max_permits(limits.max_units, options.max_running_for(&limits))
                .with_resource_limits(options.resource_limits().clone()),
            usize::MAX,
        );
        let inner = Arc::new(Inner {
            limits,
            options,
            dispatcher,
            queue,
            mailbox,
            executor,
            spawner,
            wake: Notify::new(),
            shutdown: AtomicBool::new(false),
            cancel: CancellationToken::new(),
            ledger: Arc::new(Ledger::default()),
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            _results: PhantomData,
        });
        // A permit dropped other than through `Dispatcher::complete` (a task
        // future the spawner discarded) schedules a poke, so the units it
        // frees reach the waiting task; see `poke_later`.
        inner.dispatcher.install_release_hook(|| {
            let weak = Arc::downgrade(&inner);
            move || {
                if let Some(inner) = weak.upgrade() {
                    inner.poke_later();
                }
            }
        });
        // The dispatcher counts as background work until it has stopped and
        // handed back what it held, so `shutdown_with` waits for it (without
        // reporting it as a task).
        let dispatching = Background::new(&inner.ledger);
        inner
            .spawner
            .spawn(Arc::clone(&inner).dispatch(dispatching));
        Ok(Self { inner })
    }

    /// Accept a task: store it in the durable queue, then return.
    ///
    /// Returns [`TaskStatus::Queued`] once [`LeaseQueue::enqueue`] has stored
    /// it (or found a live entry with the same idempotency key, which counts
    /// as accepted). From then on the task survives a crash of this process:
    /// it is executed and its outcome delivered to the mailbox under its
    /// [`mailbox`](TaskMetadata::mailbox) key, by this pool or after a restart.
    /// A task without a mailbox key runs, but its outcome is not stored.
    ///
    /// # Errors
    ///
    /// Nothing is stored when this fails, except where the outcome is
    /// unknown (last item):
    ///
    /// - [`SchedulerError::PoolShutdown`] after [`shutdown`](Self::shutdown);
    /// - [`SchedulerError::DeadlineExpired`] when the deadline has passed;
    /// - [`SchedulerError::CapacityExceeded`] when the task's cost exceeds the
    ///   pool's total units or a per-kind limit
    ///   ([`DurableOptions::with_resource_limit`]), so it could never start;
    /// - the queue's error: [`SchedulerError::QueueFull`] at its maximum
    ///   depth, [`SchedulerError::NotStored`] when it failed before writing
    ///   anything (both emit `Rejected`), [`SchedulerError::Backend`] when
    ///   the outcome is unknown (a connection lost mid-commit). After a
    ///   `Backend` error, retry with an idempotency key set; the retry
    ///   resolves to the stored entry. Such an error emits no `Rejected`
    ///   event (the task may run).
    pub async fn submit(&self, task: ScheduledTask<P>) -> Result<TaskStatus, SchedulerError> {
        let inner = &self.inner;
        let events = inner.events();
        let task_id = task.meta.id;
        // Before the enqueue: once stored, the dispatcher can lease and start
        // the task at any moment.
        events.submitted(&task.meta);
        if inner.shutdown.load(Ordering::Acquire) {
            let error = SchedulerError::PoolShutdown {
                pool: inner.options.name().to_owned(),
            };
            events.rejected(task_id, &error);
            return Err(error);
        }
        if task
            .meta
            .deadline_ms
            .is_some_and(|deadline| now_ms() > deadline)
        {
            let error = SchedulerError::DeadlineExpired { task_id };
            events.rejected(task_id, &error);
            return Err(error);
        }
        if let Err(error) = inner
            .dispatcher
            .check_capacity(&UnitVector::from_cost(&task.meta.cost))
        {
            events.rejected(task_id, &error);
            return Err(error);
        }
        let enqueued = inner.queue.enqueue(task).await.inspect_err(|error| {
            if refused_for_sure(error) {
                tracing::warn!(task_id, %error, "durable enqueue refused; task not accepted");
                events.rejected(task_id, error);
            } else {
                // The task may have been stored and may run (here or in
                // another process), so no terminal event: a run that happens
                // emits its own.
                tracing::warn!(
                    task_id,
                    %error,
                    "durable enqueue failed with an unknown outcome; the task may still run"
                );
            }
        })?;
        tracing::debug!(task_id, entry = %enqueued.entry(), "task persisted");
        inner.wake.notify_one();
        Ok(TaskStatus::Queued)
    }

    /// Shut the pool down under `policy` and report what happened to the
    /// tasks this process held.
    ///
    /// Leasing stops at once and the leased task waiting for capacity, if
    /// any, is handed back (`nack(Requeue)`); tasks never leased stay in the
    /// queue and are not counted. Running tasks finish, deliver and resolve
    /// their leases until the policy's drain deadline; the call returns as
    /// soon as nothing is held. Under `DrainThenCancel` and
    /// `CancelImmediately`, tasks still running at the deadline are cancelled
    /// and handed back, and the call waits up to 5 s more for that. Every
    /// handed-back task is in the report's `requeued` (also when the nack
    /// failed and the lease is left to expire, which redelivers it the same
    /// way); the next pool started on the queue runs it. That includes a
    /// task whose outcome could not be stored (its delivery kept failing), so
    /// it was handed back rather than completed.
    ///
    /// A task counts as `completed` once its outcome is stored and its lease
    /// resolved, not when its executor returns: one still delivering when the
    /// shutdown begins is counted when its delivery ends.
    ///
    /// Under `Drain`, a task still running at the deadline keeps running,
    /// heartbeat included, and resolves its lease when it finishes; it is
    /// counted as `unsettled`.
    pub async fn shutdown_with(&self, policy: ShutdownPolicy) -> ShutdownReport {
        let inner = &self.inner;
        let ledger = &inner.ledger;
        let since = ledger.tally();
        inner.request_shutdown();
        tracing::info!(
            pool = inner.options.name(),
            ?policy,
            "shutting down durable pool"
        );
        let settled = |ledger: &Ledger| ledger.in_flight() == 0 && ledger.background() == 0;
        let drained = ledger.wait_until(policy.drain_deadline(), settled).await;
        if !drained && policy.cancels_stragglers() {
            inner.cancel.cancel();
            ledger.wait_until(CANCEL_GRACE, settled).await;
        }
        let report = ledger.report(since);
        tracing::info!(
            pool = inner.options.name(),
            ?report,
            "durable pool shut down"
        );
        report
    }
}

impl<P, T, Q, M, E, S> DurableResourcePool<P, T, Q, M, E, S> {
    /// The durable queue.
    #[must_use]
    pub fn queue(&self) -> &Q {
        &self.inner.queue
    }

    /// The result mailbox: consume task outcomes here by
    /// [`MailboxKey`].
    #[must_use]
    pub fn mailbox(&self) -> &M {
        &self.inner.mailbox
    }

    /// The pool's options.
    #[must_use]
    pub fn options(&self) -> &DurableOptions {
        &self.inner.options
    }

    /// The bus the pool emits lifecycle events on (cr-31); the one set with
    /// [`DurableOptions::with_hooks`]. Register hooks on it at any time.
    #[must_use]
    pub fn hooks(&self) -> &prometheus_parking_lot_core::HookBus {
        self.inner.options.hooks()
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

    /// Executions in this process that returned a value.
    #[must_use]
    pub fn completed_tasks(&self) -> u64 {
        // Relaxed: a statistic, read for reporting only.
        self.inner.completed.load(Ordering::Relaxed)
    }

    /// Tasks settled as failures in this process: executor error or panic,
    /// timeout, expired deadline or a cost the pool can never admit.
    #[must_use]
    pub fn failed_tasks(&self) -> u64 {
        // Relaxed: a statistic, read for reporting only.
        self.inner.failed.load(Ordering::Relaxed)
    }

    /// Stop leasing, without waiting. Later submissions fail with
    /// [`SchedulerError::PoolShutdown`]; running tasks finish and resolve
    /// their leases; a leased task still waiting for capacity is handed back
    /// to the queue (nack, requeue). To bound the drain and report on it, use
    /// [`shutdown_with`](Self::shutdown_with).
    pub fn shutdown(&self) {
        self.inner.request_shutdown();
    }
}

impl<P, T, Q, M, E, S> Drop for DurableResourcePool<P, T, Q, M, E, S> {
    fn drop(&mut self) {
        self.inner.request_shutdown();
    }
}

impl<P, T, Q, M, E, S> Inner<P, T, Q, M, E, S> {
    fn events(&self) -> Events<'_> {
        Events {
            bus: self.options.hooks(),
            pool: self.options.name(),
        }
    }

    fn request_shutdown(&self) {
        // Release pairs with the dispatcher's Acquire load: everything before
        // the request happens-before the dispatcher's exit path. The
        // notification is a stored permit if the dispatcher is not waiting
        // yet, so the request cannot be missed.
        self.shutdown.store(true, Ordering::Release);
        self.wake.notify_one();
    }
}

impl<P, T, Q, M, E, S> Inner<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    Q: LeaseQueue<P> + 'static,
    M: ResultMailbox<T> + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Send + Sync + 'static,
{
    /// The dispatcher: lease the head while there is room, hand it to the
    /// core dispatcher, and sleep until woken or the poll interval elapses.
    ///
    /// No lost wakeup: every event that can make room (a completion) or work
    /// (a submission, shutdown) calls `notify_one` *after* changing the state
    /// the loop checks. `Notify` stores one permit when nobody waits, so an
    /// event that lands between the check and the `notified().await` makes
    /// that await return at once. Work from other processes and expired
    /// leases have no notification; the poll interval bounds their latency.
    async fn dispatch(self: Arc<Self>, dispatching: Background) {
        let lease_for = self.options.lease_duration();
        let poll_every = self.options.poll_interval();
        loop {
            // Acquire: pairs with `request_shutdown`.
            if self.shutdown.load(Ordering::Acquire) {
                break;
            }
            if !self.has_room() {
                self.wake.notified().await;
                continue;
            }
            match self.queue.lease(1, lease_for).await {
                Ok(batch) if batch.is_empty() => {
                    let _ = tokio::time::timeout(poll_every, self.wake.notified()).await;
                }
                Ok(batch) => {
                    for leased in batch {
                        self.admit(leased);
                    }
                }
                Err(error) => {
                    tracing::error!(pool = self.options.name(), %error, "lease failed; retrying");
                    let _ = tokio::time::timeout(poll_every, self.wake.notified()).await;
                }
            }
        }
        self.hand_back_waiting().await;
        tracing::debug!(pool = self.options.name(), "durable dispatcher stopped");
        drop(dispatching);
    }

    /// Room to lease another task: nothing leased is waiting for capacity, at
    /// least one unit is free and fewer than
    /// [`max_running`](DurableOptions::with_max_running) tasks run (the bound
    /// that holds for zero-cost tasks, which take no unit). A leased head that
    /// needs more units than are free waits in the dispatcher and blocks
    /// further leasing (strict order, as in the in-memory pool).
    fn has_room(&self) -> bool {
        self.dispatcher.parked() == 0
            && self.dispatcher.units_in_use() < self.limits.max_units
            && self.dispatcher.running() < self.options.max_running_for(&self.limits)
    }

    /// Take a leased task in: heartbeat it, then start it, let it wait for
    /// capacity, or settle it at once when it can never run.
    fn admit(self: &Arc<Self>, leased: LeasedTask<P>) {
        let LeasedTask {
            task,
            token,
            attempt,
            ..
        } = leased;
        tracing::debug!(task_id = task.meta.id, entry = %token.entry(), attempt, "leased");
        let item = LeasedItem {
            held: Held::new(&self.ledger),
            heartbeat: self.start_heartbeat(token),
            task,
            token,
            attempt,
        };
        // Leased while the pool was shutting down: give it straight back.
        // Acquire: pairs with `request_shutdown`.
        if self.shutdown.load(Ordering::Acquire) {
            let inner = Arc::clone(self);
            self.spawner
                .spawn(async move { inner.hand_back(item).await });
            return;
        }
        let now = now_ms();
        if item.task.ticket().is_expired(now) {
            self.settle_expired(item);
            return;
        }
        if let Err(error) = self
            .dispatcher
            .check_capacity(&UnitVector::from_cost(&item.task.meta.cost))
        {
            // Every consumer that leased it so far was too small: stop
            // cycling it and give its client a terminal outcome.
            if item.attempt >= self.options.oversize_attempt_limit() {
                self.settle_too_large(item, error);
                return;
            }
            // Too large for this process, not necessarily for every consumer
            // of the queue (mixed capacities, or limits lowered during a
            // rolling deploy): give it back, delayed so this process does not
            // spin on it, rather than destroy accepted work.
            let delay = self.options.lease_duration();
            tracing::warn!(
                task_id = item.task.meta.id,
                %error,
                ?delay,
                "leased task is too large for this pool; handing it back for another consumer"
            );
            let inner = Arc::clone(self);
            self.spawner.spawn(async move {
                inner
                    .hand_back_with(item, NackAction::requeue_after(delay))
                    .await;
            });
            return;
        }
        let task_id = item.task.meta.id;
        let events = self.events();
        match self
            .dispatcher
            .submit_with(item, now, |depth| events.queued(task_id, depth))
        {
            Ok(admission) => self.launch(admission.dispatched),
            // Unreachable with the checks above and an unbounded local queue.
            // The item, and with it the heartbeat, is dropped: the lease
            // expires and the task is redelivered.
            Err(error) => {
                tracing::error!(%error, "leased task not admitted; it will be redelivered");
            }
        }
    }

    /// Start every admitted task and settle every task found expired.
    fn launch(self: &Arc<Self>, dispatched: Dispatched<LeasedItem<P>>) {
        let Dispatched { started, expired } = dispatched;
        for item in expired {
            self.settle_expired(item);
        }
        for Grant { item, permit } in started {
            tracing::debug!(task_id = item.task.meta.id, "starting durable task");
            self.spawner.spawn(Arc::clone(self).supervise(item, permit));
        }
    }

    /// Run one task behind the panic backstop. A panic that escapes `run`
    /// (outside the executor containment) drops the task's permit and its
    /// heartbeat by unwinding: the freed units are handed on with a poke, and
    /// the lease expires, so the task is redelivered.
    async fn supervise(self: Arc<Self>, item: LeasedItem<P>, permit: Permit) {
        let task_id = item.task.meta.id;
        let attempt = item.attempt;
        // Set (Relaxed: one task's own flag; tokio orders its polls) just
        // before this run's terminal event is emitted.
        let reported = AtomicBool::new(false);
        if let Err(panicked) = contain(Arc::clone(&self).run(item, permit, &reported)).await {
            tracing::error!(
                task_id,
                %panicked,
                "panic escaped the durable task path; the task will be redelivered"
            );
            if !reported.load(Ordering::Relaxed) {
                self.events().requeued(task_id, None, attempt);
            }
            let woken = self.dispatcher.poke(now_ms());
            self.launch(woken);
            self.wake.notify_one();
        }
    }

    /// Execute under the budget, free the units, then deliver and resolve.
    ///
    /// A task started after the cancel phase began is handed back without
    /// running; a running one is raced against the cancel token and handed
    /// back when it fires.
    async fn run(self: Arc<Self>, item: LeasedItem<P>, permit: Permit, reported: &AtomicBool) {
        let LeasedItem {
            task,
            token,
            attempt,
            heartbeat,
            held,
        } = item;
        let ScheduledTask { meta, payload } = task;
        let task_id = meta.id;
        let idempotency_key = self.idempotency_key(&meta, token);
        if self.cancel.is_cancelled() {
            self.release(permit);
            reported.store(true, Ordering::Relaxed);
            self.requeue(task_id, &idempotency_key, attempt, token, heartbeat)
                .await;
            drop(held);
            return;
        }
        let mailbox = meta.mailbox.clone();
        let budget = self.budget(&meta);
        tracing::debug!(task_id, attempt, ?budget, "executing durable task");
        let events = self.events();
        events.started(task_id, 0);
        let started = events.clock();

        let executor = &self.executor;
        let cancel = &self.cancel;
        // Set when the cancel branch wins, before `select!` drops the
        // executor future: a panic caught afterwards came from that drop
        // (the task was cancelled), while a panic with the flag clear is the
        // executor's own, even if the token fired meanwhile. Relaxed: written
        // and read by this one task.
        let cancel_won = AtomicBool::new(false);
        let won = &cancel_won;
        let contained = contain(async move {
            tokio::select! {
                biased;
                () = async {
                    cancel.cancelled().await;
                    won.store(true, Ordering::Relaxed);
                } => None,
                ran = tokio::time::timeout(budget, executor.execute(payload, meta)) => Some(ran),
            }
        })
        .await;
        let outcome = match contained {
            Ok(None) => {
                tracing::warn!(
                    task_id,
                    "running durable task cancelled at shutdown; handing it back"
                );
                self.release(permit);
                reported.store(true, Ordering::Relaxed);
                self.requeue(task_id, &idempotency_key, attempt, token, heartbeat)
                    .await;
                drop(held);
                return;
            }
            // A panic while the cancelled executor future was dropped: the
            // task was cancelled, not failed, so it is handed back too.
            Err(panicked) if cancel_won.load(Ordering::Relaxed) => {
                tracing::warn!(
                    task_id,
                    %panicked,
                    "cancelled durable task panicked while being dropped; handing it back"
                );
                self.release(permit);
                reported.store(true, Ordering::Relaxed);
                self.requeue(task_id, &idempotency_key, attempt, token, heartbeat)
                    .await;
                drop(held);
                return;
            }
            Ok(Some(Ok(outcome))) => outcome,
            Ok(Some(Err(_elapsed))) => Err(TaskError::Timeout { budget }),
            Err(panicked) => Err(TaskError::from(panicked)),
        };

        // The units are free once the executor is done; deliver and ack are
        // storage IO and do not hold capacity.
        self.release(permit);

        let (status, payload, resolution) = match outcome {
            Ok(value) => {
                // Relaxed: statistics only.
                self.completed.fetch_add(1, Ordering::Relaxed);
                (TaskStatus::Completed, Some(value), Resolution::Ack)
            }
            Err(error) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(task_id, %error, "durable task failed");
                let reason = error.to_string();
                (
                    TaskStatus::Failed(error),
                    None,
                    Resolution::DeadLetter(reason),
                )
            }
        };
        let settlement = Settlement {
            task_id,
            mailbox,
            idempotency_key,
            status,
            payload,
            resolution,
            attempt,
            terminal: Terminal::Ran(started),
        };
        self.settle(settlement, token, heartbeat, held, Some(reported))
            .await;
    }

    /// The release hook's job: spawn a drain and start what it admits, then
    /// wake the dispatcher (freed units may let it lease more). Spawned,
    /// because the dropping thread may hold this pool's locks.
    fn poke_later(self: &Arc<Self>) {
        let inner = Arc::clone(self);
        self.spawner.spawn(async move {
            let woken = inner.dispatcher.poke(now_ms());
            inner.launch(woken);
            inner.wake.notify_one();
        });
    }

    /// Return a permit and start what the freed units admit.
    fn release(self: &Arc<Self>, permit: Permit) {
        let woken = self.dispatcher.complete(permit, now_ms());
        self.launch(woken);
        self.wake.notify_one();
    }

    /// Settle a leased task whose deadline passed before it could start:
    /// deliver `Expired` and dead-letter it. Every consumer would refuse it
    /// (as `submit` does), so it is not handed back.
    fn settle_expired(self: &Arc<Self>, item: LeasedItem<P>) {
        let LeasedItem {
            task,
            token,
            heartbeat,
            held,
            attempt,
        } = item;
        let reason = "deadline passed before the task started";
        self.failed.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(task_id = task.meta.id, reason, "durable task cannot run");
        let settlement = Settlement {
            task_id: task.meta.id,
            mailbox: task.meta.mailbox.clone(),
            idempotency_key: self.idempotency_key(&task.meta, token),
            status: TaskStatus::Expired,
            payload: None,
            resolution: Resolution::DeadLetter(reason.to_owned()),
            attempt,
            terminal: Terminal::Expired,
        };
        let inner = Arc::clone(self);
        self.spawner.spawn(async move {
            inner.settle(settlement, token, heartbeat, held, None).await;
        });
    }

    /// Settle a leased task too large for this pool that reached the
    /// oversize attempt limit: deliver `Dropped`, emit `Rejected` and
    /// dead-letter it. Every consumer that leased it so far (at least
    /// `limit - 1` hand-backs, one lease duration apart) was too small.
    fn settle_too_large(self: &Arc<Self>, item: LeasedItem<P>, error: SchedulerError) {
        let LeasedItem {
            task,
            token,
            heartbeat,
            held,
            attempt,
        } = item;
        let limit = self.options.oversize_attempt_limit();
        let reason = format!(
            "{error}; still too large for the consumer that leased it at attempt {attempt} \
             (oversize attempt limit {limit})"
        );
        self.failed.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(
            task_id = task.meta.id,
            %error,
            attempt,
            limit,
            "leased task is too large for every consumer that leased it; dropping it"
        );
        let settlement = Settlement {
            task_id: task.meta.id,
            mailbox: task.meta.mailbox.clone(),
            idempotency_key: self.idempotency_key(&task.meta, token),
            status: TaskStatus::Dropped(reason.clone()),
            payload: None,
            resolution: Resolution::DeadLetter(reason),
            attempt,
            terminal: Terminal::TooLarge(error),
        };
        let inner = Arc::clone(self);
        self.spawner.spawn(async move {
            inner.settle(settlement, token, heartbeat, held, None).await;
        });
    }

    /// Deliver, then resolve the lease. The ordering is the durability
    /// guarantee: the lease is acked (or dead-lettered) only after the
    /// mailbox has stored the outcome, so a crash between the two re-runs the
    /// task and the idempotent delivery keeps one result.
    ///
    /// The run's terminal event is emitted once the outcome is stored (or the
    /// task handed back), and `reported` set just before it. `held` is
    /// settled once the lease is resolved: as finished when the outcome was
    /// stored, as requeued when the task was handed back.
    async fn settle(
        &self,
        settlement: Settlement<T>,
        token: LeaseToken,
        heartbeat: Heartbeat,
        held: Held,
        reported: Option<&AtomicBool>,
    ) {
        let Settlement {
            task_id,
            mailbox,
            idempotency_key,
            status,
            payload,
            resolution,
            attempt,
            terminal,
        } = settlement;
        let report = || {
            if let Some(reported) = reported {
                reported.store(true, Ordering::Relaxed);
            }
        };

        // Negative control S-16: resolve the lease before the delivery. A
        // crash between the two then loses the result, and the
        // `pool_sigkill_*_between_execution_and_delivery` tests must fail.
        #[cfg(parking_lot_negative_control = "s-16")]
        self.resolve(task_id, token, &resolution).await;

        let status = if let Some(key) = mailbox {
            let mut delivery =
                Delivery::new(key, task_id, status, payload).with_idempotency_key(idempotency_key);
            if let Some(ttl) = self.options.result_ttl() {
                delivery = delivery.with_ttl(ttl);
            }
            if !self.deliver(task_id, &delivery).await {
                // Never ack an undelivered outcome: hand the task back so a
                // later attempt delivers it. If the nack fails too, the lease
                // expires once the heartbeat stops.
                report();
                self.events()
                    .requeued(task_id, delivery.idempotency_key.as_deref(), attempt);
                drop(heartbeat);
                if let Err(error) = self.queue.nack(token, NackAction::requeue()).await {
                    tracing::warn!(task_id, %error, "nack failed; the lease will expire instead");
                }
                // Settled as requeued: the task runs again.
                drop(held);
                return;
            }
            delivery.status
        } else {
            status
        };

        report();
        self.emit_terminal(task_id, &terminal, &status);

        // Stop extending before resolving, so no heartbeat races the ack. The
        // last extension left at least `lease - heartbeat interval` of lease.
        drop(heartbeat);
        #[cfg(not(parking_lot_negative_control = "s-16"))]
        self.resolve(task_id, token, &resolution).await;
        // The outcome is stored: a lease that could not be resolved expires
        // and the redelivery stores nothing new.
        held.finish();
    }

    /// Emit a settled run's terminal event.
    fn emit_terminal(&self, task_id: TaskId, terminal: &Terminal, status: &TaskStatus) {
        let events = self.events();
        match (terminal, status) {
            (Terminal::Expired, _) => events.expired(task_id),
            (Terminal::TooLarge(error), _) => events.rejected(task_id, error),
            (Terminal::Ran(_), TaskStatus::Failed(error)) => events.failed(task_id, error),
            (Terminal::Ran(started), _) => events.completed(task_id, *started),
        }
    }

    /// Store the delivery, retrying with backoff. True once it is stored (or
    /// was already stored by an earlier attempt of the same task).
    async fn deliver(&self, task_id: TaskId, delivery: &Delivery<T>) -> bool {
        let mut backoff = FIRST_BACKOFF;
        for attempt in 1..=ATTEMPTS {
            match self.mailbox.deliver(delivery.clone()).await {
                Ok(outcome) => {
                    tracing::debug!(task_id, ?outcome, "outcome delivered");
                    return true;
                }
                Err(error) if attempt < ATTEMPTS => {
                    tracing::warn!(task_id, attempt, %error, "delivery failed; retrying");
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
                Err(error) => {
                    tracing::error!(task_id, %error, "delivery failed; handing the task back");
                }
            }
        }
        false
    }

    /// Ack or dead-letter the lease, retrying a backend failure. A lost lease
    /// is final: the entry's current holder resolves it, and this attempt's
    /// result is already deduplicated by the mailbox.
    async fn resolve(&self, task_id: TaskId, token: LeaseToken, resolution: &Resolution) {
        let mut backoff = FIRST_BACKOFF;
        for attempt in 1..=ATTEMPTS {
            let result = match resolution {
                Resolution::Ack => self.queue.ack(token).await,
                Resolution::DeadLetter(reason) => {
                    self.queue
                        .nack(token, NackAction::dead_letter(reason.clone()))
                        .await
                }
            };
            match result {
                Ok(()) => return,
                Err(SchedulerError::LeaseLost { entry }) => {
                    tracing::info!(
                        task_id,
                        entry,
                        "lease lost before it was resolved; another consumer holds the entry"
                    );
                    return;
                }
                Err(error) if attempt < ATTEMPTS => {
                    tracing::warn!(task_id, attempt, %error, "resolving the lease failed; retrying");
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
                Err(error) => tracing::error!(
                    task_id,
                    %error,
                    "resolving the lease failed; it will expire and the task will be redelivered"
                ),
            }
        }
    }

    /// Spawn the heartbeat for `token`; it runs until the returned guard is
    /// dropped or the lease is lost.
    fn start_heartbeat(self: &Arc<Self>, token: LeaseToken) -> Heartbeat {
        let (stop, stopped) = oneshot::channel();
        self.spawner
            .spawn(Arc::clone(self).heartbeat(token, stopped));
        Heartbeat { _stop: stop }
    }

    /// Extend the lease every heartbeat interval.
    ///
    /// The extension itself is raced against the stop signal, so dropping the
    /// guard cancels an extension in flight; the contract makes that safe (the
    /// lease keeps whichever expiry it had).
    async fn heartbeat(self: Arc<Self>, token: LeaseToken, mut stopped: oneshot::Receiver<()>) {
        let every = self.options.heartbeat_interval();
        let lease_for = self.options.lease_duration();
        let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                biased;
                _ = &mut stopped => return,
                _ = ticker.tick() => {}
            }
            tokio::select! {
                biased;
                _ = &mut stopped => return,
                extended = self.queue.extend(token, lease_for) => match extended {
                    Ok(()) => {}
                    Err(SchedulerError::LeaseLost { entry }) => {
                        tracing::warn!(
                            entry,
                            "lease lost while the task was held; it keeps running and its \
                             result is delivered idempotently"
                        );
                        return;
                    }
                    Err(error) => {
                        tracing::warn!(entry = %token.entry(), %error, "heartbeat failed; retrying at the next beat");
                    }
                },
            }
        }
    }

    /// On shutdown, give back the leased task that is still waiting for
    /// capacity, so another consumer can take it at once.
    async fn hand_back_waiting(&self) {
        for item in self.dispatcher.take_parked() {
            self.hand_back(item).await;
        }
    }

    /// Give a leased task that will not run here back to the queue.
    async fn hand_back(&self, item: LeasedItem<P>) {
        self.hand_back_with(item, NackAction::requeue()).await;
    }

    /// Give a leased task back to the queue with `action` (a requeue).
    async fn hand_back_with(&self, item: LeasedItem<P>, action: NackAction) {
        let LeasedItem {
            task,
            token,
            heartbeat,
            held,
            attempt,
        } = item;
        let idempotency_key = self.idempotency_key(&task.meta, token);
        self.requeue_with(
            task.meta.id,
            &idempotency_key,
            attempt,
            token,
            heartbeat,
            action,
        )
        .await;
        drop(held);
    }

    /// Stop the heartbeat, then `nack(Requeue)` the lease, retrying a backend
    /// failure. The caller drops the task's `Held` afterwards, which counts it
    /// as requeued in every case: a lost lease is already back in the queue
    /// (or held by its new consumer), and a nack that keeps failing leaves the
    /// lease to expire, which redelivers it the same way.
    ///
    /// Emits the run's `Requeued` event first.
    async fn requeue(
        &self,
        task_id: TaskId,
        idempotency_key: &str,
        attempt: u32,
        token: LeaseToken,
        heartbeat: Heartbeat,
    ) {
        self.requeue_with(
            task_id,
            idempotency_key,
            attempt,
            token,
            heartbeat,
            NackAction::requeue(),
        )
        .await;
    }

    /// [`requeue`](Self::requeue) with a chosen requeue action.
    async fn requeue_with(
        &self,
        task_id: TaskId,
        idempotency_key: &str,
        attempt: u32,
        token: LeaseToken,
        heartbeat: Heartbeat,
        action: NackAction,
    ) {
        self.events()
            .requeued(task_id, Some(idempotency_key), attempt);
        drop(heartbeat);
        let mut backoff = FIRST_BACKOFF;
        for attempt in 1..=ATTEMPTS {
            match self.queue.nack(token, action.clone()).await {
                Ok(()) => break,
                Err(SchedulerError::LeaseLost { entry }) => {
                    tracing::info!(
                        task_id,
                        entry,
                        "lease already lost when handing the task back"
                    );
                    break;
                }
                Err(error) if attempt < ATTEMPTS => {
                    tracing::warn!(task_id, attempt, %error, "nack failed; retrying");
                    tokio::time::sleep(backoff).await;
                    backoff *= 2;
                }
                Err(error) => tracing::warn!(
                    task_id,
                    %error,
                    "nack failed; the lease will expire and the task will be redelivered"
                ),
            }
        }
    }

    /// The delivery's idempotency key: the caller's key, or one derived from
    /// the queue's identity and the entry, which is the same for every
    /// redelivery of the entry whichever pool (whatever its name) leases it.
    fn idempotency_key(&self, meta: &TaskMetadata, token: LeaseToken) -> String {
        meta.idempotency_key
            .clone()
            .unwrap_or_else(|| format!("{}:entry:{}", self.queue.identity(), token.entry()))
    }

    /// The execution budget: the task's deadline when nearer than the pool's
    /// `default_timeout`, otherwise the default (as in the in-memory pool).
    fn budget(&self, meta: &TaskMetadata) -> Duration {
        let Some(deadline) = meta.deadline_ms else {
            return self.limits.default_timeout;
        };
        let remaining = deadline.saturating_sub(now_ms());
        Duration::from_millis(u64::try_from(remaining).unwrap_or(u64::MAX))
            .min(self.limits.default_timeout)
    }
}
