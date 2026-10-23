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
//! | cost exceeds the pool's total capacity | `Dropped(reason)` | dead-letter |
//! | delivery still failing after retries | nothing | nack (requeue) |
//! | pool shut down before the task started | nothing | nack (requeue) |
//! | process killed at any point | nothing new | expires, redelivered |
//!
//! Failures are final, as in the in-memory pool (no automatic retry); they are
//! dead-lettered rather than acked so operators keep a record of them in the
//! queue. A task that kills its process is redelivered on every restart; cap
//! that with the backend's own attempt limit (for example
//! `PgLeaseQueue::with_max_attempts`).
//!
//! # Exactly one consumable result
//!
//! Every delivery carries an idempotency key, and the mailbox keeps the first
//! write for a key (ADR 0003), including after it was consumed:
//!
//! - the task's own [`idempotency_key`](TaskMetadata::idempotency_key) when
//!   the caller set one, so a client that resubmits a finished task gets no
//!   second result either;
//! - otherwise `"<pool name>:entry:<queue entry id>"`. The entry id is
//!   assigned once at enqueue and travels with every redelivery of the entry
//!   (it is the [`LeaseToken::entry`]), so every execution of one accepted
//!   task delivers under one key.
//!
//! A redelivered task that already delivered its result therefore runs again
//! but stores nothing new: the consumer sees exactly one result.
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
//! # Ordering difference from the in-memory pool
//!
//! At most one leased task waits for capacity at a time. A task enqueued at a
//! higher priority after that head was leased waits behind it; in the
//! in-memory pool it would overtake the parked head.

mod options;

use std::marker::PhantomData;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use prometheus_parking_lot_core::dispatch::{Dispatched, Dispatcher, Grant, Parkable, Ticket};
use prometheus_parking_lot_core::semaphore::{Permit, UnitSemaphore};
use prometheus_parking_lot_core::{
    Delivery, LeaseQueue, LeaseToken, LeasedTask, MailboxKey, NackAction, PoolLimits,
    ResultMailbox, ScheduledTask, SchedulerError, Spawn, TaskError, TaskExecutor, TaskId,
    TaskMetadata, TaskPayload, TaskStatus, now_ms,
};
use tokio::sync::{Notify, oneshot};

use crate::contain::contain;

pub use self::options::DurableOptions;

/// Attempts at a mailbox delivery, or at an ack or nack, before giving up.
const ATTEMPTS: u32 = 5;
/// Delay after the first failed attempt; doubles after each further one.
const FIRST_BACKOFF: Duration = Duration::from_millis(100);

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
    /// poll interval or an empty name.
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
            UnitSemaphore::new(limits.max_units),
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
            completed: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            _results: PhantomData,
        });
        inner.spawner.spawn(Arc::clone(&inner).dispatch());
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
    /// Nothing is stored when this fails:
    ///
    /// - [`SchedulerError::PoolShutdown`] after [`shutdown`](Self::shutdown);
    /// - [`SchedulerError::DeadlineExpired`] when the deadline has passed;
    /// - [`SchedulerError::CapacityExceeded`] when the task's cost exceeds the
    ///   pool's total units, so it could never start;
    /// - the queue's error: [`SchedulerError::QueueFull`] at its maximum
    ///   depth, [`SchedulerError::Backend`] when it cannot store the task. If
    ///   the error leaves the outcome unknown (a connection lost mid-commit),
    ///   retry with an idempotency key set; the retry resolves to the stored
    ///   entry.
    pub async fn submit(&self, task: ScheduledTask<P>) -> Result<TaskStatus, SchedulerError> {
        let inner = &self.inner;
        if inner.shutdown.load(Ordering::Acquire) {
            return Err(SchedulerError::PoolShutdown {
                pool: inner.options.name().to_owned(),
            });
        }
        let task_id = task.meta.id;
        if task
            .meta
            .deadline_ms
            .is_some_and(|deadline| now_ms() > deadline)
        {
            return Err(SchedulerError::DeadlineExpired { task_id });
        }
        let units = task.meta.cost.total_units();
        if units > inner.limits.max_units {
            return Err(SchedulerError::CapacityExceeded {
                pool: inner.options.name().to_owned(),
                requested: units,
                available: inner.limits.max_units,
            });
        }
        let enqueued = inner.queue.enqueue(task).await.inspect_err(|error| {
            tracing::warn!(task_id, %error, "durable enqueue failed; task not accepted");
        })?;
        tracing::debug!(task_id, entry = %enqueued.entry(), "task persisted");
        inner.wake.notify_one();
        Ok(TaskStatus::Queued)
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

    /// Stop leasing. Later submissions fail with
    /// [`SchedulerError::PoolShutdown`]; running tasks finish and resolve their
    /// leases; a leased task still waiting for capacity is handed back to the
    /// queue (nack, requeue). Draining policy is cr-29.
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
    async fn dispatch(self: Arc<Self>) {
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
    }

    /// Room to lease another task: nothing leased is waiting for capacity, and
    /// at least one unit is free. A leased head that needs more units than are
    /// free waits in the dispatcher and blocks further leasing (strict order,
    /// as in the in-memory pool).
    fn has_room(&self) -> bool {
        self.dispatcher.parked() == 0 && self.dispatcher.units_in_use() < self.limits.max_units
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
            heartbeat: self.start_heartbeat(token),
            task,
            token,
            attempt,
        };
        let now = now_ms();
        if item.task.ticket().is_expired(now) {
            self.settle_unrun(
                item,
                TaskStatus::Expired,
                "deadline passed before the task started",
            );
            return;
        }
        let units = item.task.meta.cost.total_units();
        if units > self.limits.max_units {
            let reason = format!(
                "cost of {units} units exceeds the pool's {} units",
                self.limits.max_units
            );
            self.settle_unrun(item, TaskStatus::Dropped(reason.clone()), &reason);
            return;
        }
        match self.dispatcher.submit(item, now) {
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
            self.settle_unrun(
                item,
                TaskStatus::Expired,
                "deadline passed before the task started",
            );
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
        if let Err(panicked) = contain(Arc::clone(&self).run(item, permit)).await {
            tracing::error!(
                task_id,
                %panicked,
                "panic escaped the durable task path; the task will be redelivered"
            );
            let woken = self.dispatcher.poke(now_ms());
            self.launch(woken);
            self.wake.notify_one();
        }
    }

    /// Execute under the budget, free the units, then deliver and resolve.
    async fn run(self: Arc<Self>, item: LeasedItem<P>, permit: Permit) {
        let LeasedItem {
            task,
            token,
            attempt,
            heartbeat,
        } = item;
        let ScheduledTask { meta, payload } = task;
        let task_id = meta.id;
        let mailbox = meta.mailbox.clone();
        let idempotency_key = self.idempotency_key(&meta, token);
        let budget = self.budget(&meta);
        tracing::debug!(task_id, attempt, ?budget, "executing durable task");

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

        // The units are free once the executor is done; deliver and ack are
        // storage IO and do not hold capacity.
        let woken = self.dispatcher.complete(permit, now_ms());
        self.launch(woken);
        self.wake.notify_one();

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
        };
        self.settle(settlement, token, heartbeat).await;
    }

    /// Settle a leased task that never ran (expired, or too large for the
    /// pool): deliver its status and dead-letter it.
    fn settle_unrun(self: &Arc<Self>, item: LeasedItem<P>, status: TaskStatus, reason: &str) {
        let LeasedItem {
            task,
            token,
            heartbeat,
            ..
        } = item;
        self.failed.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(task_id = task.meta.id, reason, "durable task cannot run");
        let settlement = Settlement {
            task_id: task.meta.id,
            mailbox: task.meta.mailbox.clone(),
            idempotency_key: self.idempotency_key(&task.meta, token),
            status,
            payload: None,
            resolution: Resolution::DeadLetter(reason.to_owned()),
        };
        let inner = Arc::clone(self);
        self.spawner
            .spawn(async move { inner.settle(settlement, token, heartbeat).await });
    }

    /// Deliver, then resolve the lease. The ordering is the durability
    /// guarantee: the lease is acked (or dead-lettered) only after the
    /// mailbox has stored the outcome, so a crash between the two re-runs the
    /// task and the idempotent delivery keeps one result.
    async fn settle(&self, settlement: Settlement<T>, token: LeaseToken, heartbeat: Heartbeat) {
        let Settlement {
            task_id,
            mailbox,
            idempotency_key,
            status,
            payload,
            resolution,
        } = settlement;

        // Negative control S-16: resolve the lease before the delivery. A
        // crash between the two then loses the result, and the
        // `pool_sigkill_*_between_execution_and_delivery` tests must fail.
        #[cfg(parking_lot_negative_control = "s-16")]
        self.resolve(task_id, token, &resolution).await;

        if let Some(key) = mailbox {
            let mut delivery =
                Delivery::new(key, task_id, status, payload).with_idempotency_key(idempotency_key);
            if let Some(ttl) = self.options.result_ttl() {
                delivery = delivery.with_ttl(ttl);
            }
            if !self.deliver(task_id, delivery).await {
                // Never ack an undelivered outcome: hand the task back so a
                // later attempt delivers it. If the nack fails too, the lease
                // expires once the heartbeat stops.
                drop(heartbeat);
                if let Err(error) = self.queue.nack(token, NackAction::requeue()).await {
                    tracing::warn!(task_id, %error, "nack failed; the lease will expire instead");
                }
                return;
            }
        }

        // Stop extending before resolving, so no heartbeat races the ack. The
        // last extension left at least `lease - heartbeat interval` of lease.
        drop(heartbeat);
        #[cfg(not(parking_lot_negative_control = "s-16"))]
        self.resolve(task_id, token, &resolution).await;
    }

    /// Store the delivery, retrying with backoff. True once it is stored (or
    /// was already stored by an earlier attempt of the same task).
    async fn deliver(&self, task_id: TaskId, delivery: Delivery<T>) -> bool {
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
            let LeasedItem {
                token, heartbeat, ..
            } = item;
            drop(heartbeat);
            if let Err(error) = self.queue.nack(token, NackAction::requeue()).await {
                tracing::warn!(entry = %token.entry(), %error, "nack at shutdown failed; the lease will expire instead");
            }
        }
    }

    /// The delivery's idempotency key: the caller's key, or one derived from
    /// the queue entry, which is the same for every redelivery of the entry.
    fn idempotency_key(&self, meta: &TaskMetadata, token: LeaseToken) -> String {
        meta.idempotency_key
            .clone()
            .unwrap_or_else(|| format!("{}:entry:{}", self.options.name(), token.entry()))
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
