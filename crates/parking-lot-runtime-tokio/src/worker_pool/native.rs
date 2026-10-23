//! Native implementation of `WorkerPool` using OS threads.
//!
//! This implementation spawns dedicated OS threads that each have their own
//! single-threaded tokio runtime. This ensures CPU/GPU-bound work does not
//! block the main async runtime.
//!
//! # Design Principles
//!
//! - **One dispatcher**: admission goes through the core
//!   [`Dispatcher`](prometheus_parking_lot_core::dispatch::Dispatcher) shared
//!   with `ResourcePool`. It enforces `max_units` (audit C1: the 0.2 pool
//!   ignored it), serves parked tasks by priority then submission order, and
//!   bounds admitted tasks by the worker count, so an admitted task always has
//!   a worker and `max_queue_depth` bounds exactly the tasks waiting. A
//!   parked task that fits may skip ahead of a head that does not, at most
//!   [`WorkerPoolConfig::max_head_skips`] times per head (cr-20).
//! - **Bounded execution**: every execution runs under a budget (the task's
//!   deadline or the pool's default timeout, whichever is nearer); a task
//!   still running at its budget is cancelled, its units are released and its
//!   result is [`TaskError::Timeout`].
//! - **No polling, no parked waiters**: workers block on a channel of
//!   admitted tasks; results go to per-task slots (`super::slots`, cr-22)
//!   that async callers await without holding a thread. Unretrieved results
//!   are evicted after the result TTL (default: the pool's task timeout).
//! - **Shutdown without loss** (cr-29): `shutdown_with` applies a
//!   [`ShutdownPolicy`]. During the drain, admitted tasks keep running and
//!   parked ones keep starting; at the deadline the parked remainder, any
//!   admitted task a worker receives from then on, and (unless the policy is
//!   `Drain`) running stragglers are cancelled: their result slot gets
//!   [`TaskError::Cancelled`] and a [`CancelledTask`] goes to the pool's
//!   [`CancellationSink`] from a dedicated sink thread. 0.2 dropped a task a
//!   worker received after the shutdown flag (no result, no capture); the
//!   negative control `--cfg parking_lot_negative_control="s-9"` restores
//!   that, and the `shutdown_` accounting tests must then fail.
//! - **Panic containment** (cr-21): an executor panic fails its task with
//!   [`TaskError::ExecutorPanicked`], returns its units through the
//!   dispatcher and leaves the worker serving. A panic that gets past that
//!   (see `crate::contain`) is caught per task on the worker: the task is
//!   reported as panicked if it had no outcome yet, the units its permit
//!   freed while unwinding are handed to parked work with `Dispatcher::poke`,
//!   and the worker thread is replaced by a fresh one, so the pool keeps its
//!   worker count.
//! - **Lifecycle events** (cr-31): hooks registered with
//!   [`WorkerPool::with_hook`] see, per task (keyed by
//!   [`TaskMetadata::id`]): `Submitted` first in `submit`; `Rejected` when
//!   `submit` refuses it; `Queued` under the dispatch lock when it parks;
//!   `Started` on the worker thread that runs it (`worker_id` is that
//!   worker's index); then one terminal event: `Completed`/`Failed` when the
//!   executor returns (before its units are returned), `Expired` when it
//!   reached the head after its deadline, or `Cancelled` when a shutdown or a
//!   drop captured it. A panic escaping the containment reports `Failed` from
//!   the backstop unless the terminal event was already out.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use prometheus_parking_lot_core::config::WorkerPoolConfig;
use prometheus_parking_lot_core::dispatch::{
    Dispatched, Dispatcher, Grant, PoolCounters, SkipAheadStats, WorkerTask, generate_mailbox_key,
    mailbox_key_to_string,
};
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{
    Clock, Hook, HookBus, MailboxKey, PoolStats, ResourceKind, SchedulerError, SystemClock,
    TaskError, TaskMetadata, WorkerExecutor,
};

use super::slots::{Missing, ResultSlots, SlotId};
use crate::cancellation::{
    CancelReason, CancellationConfig, CancellationSink, CancelledTask, LoggingCancellationSink,
    offer,
};
use crate::contain::{Panicked, catch, contain};
use crate::events::Events;
use crate::ledger::Ledger;
use crate::shutdown::{ShutdownPolicy, ShutdownReport};

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

/// The error for a result that did not arrive within the caller's wait.
const fn wait_timed_out() -> SchedulerError {
    SchedulerError::Timeout { task_id: None }
}

/// The error for a result evicted unread after its TTL.
fn result_expired(key: &MailboxKey) -> SchedulerError {
    SchedulerError::ResultExpired {
        key: Some(mailbox_key_to_string(key)),
    }
}

/// Map a wait that produced no value to the caller's error.
fn missing_error(key: &MailboxKey, missing: Missing) -> SchedulerError {
    match missing {
        Missing::NotFound => result_not_found(key),
        Missing::Expired => result_expired(key),
        Missing::TimedOut => wait_timed_out(),
    }
}

/// The slot id of a key issued by this pool (`generate_mailbox_key`), or
/// `None` for any other key.
fn slot_id(key: &MailboxKey) -> Option<SlotId> {
    if key.tenant != POOL_NAME || key.user_id.is_some() {
        return None;
    }
    key.session_id.as_deref()?.parse().ok()
}

/// Shortest interval at which an idle worker sweeps expired results, so a
/// tiny TTL cannot turn the idle wait into a spin.
const MIN_SWEEP_INTERVAL: Duration = Duration::from_millis(50);

/// An admitted task on its way to a worker.
type Admitted<P> = Grant<WorkerTask<P>>;

/// Extra wait, beyond the sink's `delta`, for cancelled stragglers to settle.
const SETTLE_MARGIN: Duration = Duration::from_secs(1);

/// How long `shutdown_with` waits for the worker threads to exit once the
/// policy's steps are done; a worker still running then is detached (0.2's
/// per-worker join bound).
const WORKER_EXIT_BOUND: Duration = Duration::from_secs(2);

/// How far one task got on a worker, for the panic backstop.
#[derive(Debug, Default)]
struct Progress {
    /// The task's outcome (or its capture) has been started.
    reported: bool,
    /// The task has been settled in the ledger (or handed to the sink thread,
    /// which settles it).
    settled: bool,
}

/// State shared by the pool handle and its worker threads.
struct Shared<P, R, E> {
    /// Pool configuration.
    config: WorkerPoolConfig,
    /// Admission, the parked queue and the semaphore.
    ///
    /// No release hook (`Dispatcher::install_release_hook`): every permit
    /// goes back through `complete` (a finished, cancelled or captured task,
    /// a grant `launch` could not send, the channel drain of a worker that is
    /// not replaced) or is followed by a `poke` (the panic backstop in
    /// `execute_supervised`). The only grants dropped otherwise are those
    /// still in the worker channel when the channel itself is dropped with
    /// the pool, when nothing is left to start.
    dispatcher: Dispatcher<WorkerTask<P>>,
    /// Per-task result slots with TTL eviction.
    results: ResultSlots<TaskOutcome<R>>,
    /// Submitted/completed/failed counters (running and parked come from the
    /// dispatcher).
    counters: PoolCounters,
    /// Admitted tasks to the workers. `None` once the pool shuts down; the
    /// workers exit when it is gone and the channel is empty.
    ready_tx: Mutex<Option<Sender<Admitted<P>>>>,
    /// Time source for deadlines and budgets.
    clock: RwLock<Arc<dyn Clock>>,
    /// Set by `shutdown_with`/`Drop`: submissions are refused.
    shutdown: AtomicBool,
    /// Set when the parked queue is taken at the drain deadline: a submission
    /// that raced it captures what it parked.
    parked_closed: AtomicBool,
    /// Fired at the drain deadline (policies that cancel stragglers): running
    /// tasks stop at their next await point, and a task a worker receives from
    /// then on is captured without running.
    cancel: CancellationToken,
    /// Accepted tasks until settled; shared with the sink thread.
    ledger: Arc<Ledger>,
    /// Where cancelled tasks go (`None`: a [`LoggingCancellationSink`]).
    sink: RwLock<Option<Arc<dyn CancellationSink<P>>>>,
    /// The sink's per-call bound (`delta`).
    cancellation: RwLock<CancellationConfig>,
    /// Cancelled tasks to the sink thread, started on the first capture.
    /// Dropped with `Shared`, which ends the thread once it has drained.
    capture_tx: Mutex<Option<UnboundedSender<CancelledTask<P>>>>,
    /// Lifecycle-event observers (cr-31).
    hooks: HookBus,
    /// The executor; each worker runs a clone.
    executor: E,
    /// Worker thread handles, joined by `shutdown`. A replacement worker's
    /// handle is pushed here too.
    workers: Mutex<Vec<JoinHandle<()>>>,
    /// Worker threads serving the pool. A worker that dies is replaced and
    /// its replacement takes its place, so a death does not change this; it
    /// drops when a worker exits (shutdown) or cannot be replaced.
    live_workers: AtomicUsize,
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

    const fn events(&self) -> Events<'_> {
        Events {
            bus: &self.hooks,
            pool: POOL_NAME,
        }
    }

    /// The pool clock's monotonic reading, which result TTLs are measured on.
    fn monotonic(&self) -> Duration {
        self.clock.read().monotonic()
    }

    /// How long an idle worker waits for work before sweeping expired
    /// results: the result TTL, but at least [`MIN_SWEEP_INTERVAL`].
    fn sweep_interval(&self) -> Duration {
        self.results.ttl().max(MIN_SWEEP_INTERVAL)
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
    /// unbounded), so the `budget_` wedged-executor tests must fail.
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
        let stored =
            slot_id(key).is_some_and(|id| self.results.fill(id, outcome, self.monotonic()));
        if !stored {
            error!(key = %mailbox_key_to_string(key), "no result slot for a finished task");
        }
    }

    /// Hand every admitted task to the workers and fail every task found
    /// expired at the head of the queue.
    ///
    /// A task admitted after the workers' channel was closed cannot run: its
    /// units are returned through the dispatcher and it is captured as never
    /// started (and so is anything that return admits).
    fn launch(&self, dispatched: Dispatched<WorkerTask<P>>) {
        let mut pending = dispatched;
        while !pending.is_empty() {
            let Dispatched { started, expired } = std::mem::take(&mut pending);
            for task in expired {
                warn!(
                    task_id = task.meta.id,
                    "queued task expired before it could start"
                );
                self.events().expired(task.meta.id);
                self.finish(&task.mailbox_key, Err(TaskError::DeadlineExpired));
                self.ledger.finish(1);
                self.ledger.settle(1);
            }
            if started.is_empty() {
                continue;
            }
            // A clone sent after shutdown took the original still delivers;
            // the worker that receives it runs or captures it.
            let ready_tx = self.ready_tx.lock().clone();
            for admitted in started {
                let refused = match ready_tx.as_ref() {
                    Some(tx) => tx.send(admitted).err().map(|error| error.0),
                    None => Some(admitted),
                };
                if let Some(Grant { item: task, permit }) = refused {
                    let woken = self.dispatcher.complete(permit, self.now_ms());
                    self.capture_unstarted(task);
                    pending.started.extend(woken.started);
                    pending.expired.extend(woken.expired);
                }
            }
        }
    }

    /// Capture every parked task (drain deadline, drop, or a submission that
    /// raced the shutdown): nothing parked is dropped silently.
    fn capture_parked(&self) {
        for task in self.dispatcher.take_parked() {
            self.capture_unstarted(task);
        }
    }

    /// Capture a task that never started, with its payload.
    fn capture_unstarted(&self, task: WorkerTask<P>) {
        let WorkerTask {
            payload,
            meta,
            mailbox_key,
        } = task;
        self.capture(
            meta,
            &mailbox_key,
            Some(payload),
            CancelReason::QueueShutdown,
        );
    }

    /// Record a cancelled task: fail its result slot with
    /// [`TaskError::Cancelled`] and hand a [`CancelledTask`] to the sink
    /// thread, which offers it to the sink under `delta` and then settles it.
    /// Never blocks, so it is safe on any thread.
    fn capture(
        &self,
        meta: TaskMetadata,
        mailbox_key: &MailboxKey,
        payload: Option<P>,
        reason: CancelReason,
    ) {
        let task_id = meta.id;
        warn!(task_id, ?reason, "task cancelled at shutdown");
        self.events().cancelled(&meta, &reason, 0);
        self.finish(mailbox_key, Err(shutdown_cancelled()));
        self.ledger.begin_capture();
        let cancelled = CancelledTask {
            payload,
            meta,
            reason,
            attempt: 0,
            cancelled_at_ms: u64::try_from(self.now_ms()).unwrap_or(u64::MAX),
        };
        let unsent = {
            let mut capture_tx = self.capture_tx.lock();
            if capture_tx.is_none() {
                let sink = self.sink.read().clone().unwrap_or_else(|| {
                    Arc::new(LoggingCancellationSink::new(POOL_NAME))
                        as Arc<dyn CancellationSink<P>>
                });
                let delta = self.cancellation.read().delta;
                match spawn_sink_thread(sink, delta, Arc::clone(&self.ledger)) {
                    Ok(tx) => *capture_tx = Some(tx),
                    Err(spawn_error) => {
                        error!(error = %spawn_error, "could not start the cancellation sink thread");
                    }
                }
            }
            match capture_tx.as_ref() {
                Some(tx) => tx.send(cancelled).err().map(|error| error.0),
                None => Some(cancelled),
            }
        };
        if let Some(cancelled) = unsent {
            error!(
                task_id = cancelled.meta.id,
                "cancellation sink unavailable; the cancelled task is not recorded"
            );
            self.ledger.end_capture(task_id, None);
            self.ledger.settle(1);
        }
    }

    /// Run one admitted task on the calling worker behind the panic backstop.
    ///
    /// Returns `Err` when a panic escaped [`execute`](Self::execute)'s own
    /// containment; the worker must then be replaced. Before returning, the
    /// task is reported as panicked unless its completion had started, and
    /// the units its permit freed while unwinding are handed to parked work.
    fn execute_supervised(
        &self,
        runtime: &tokio::runtime::Runtime,
        admitted: Admitted<P>,
        worker_id: usize,
    ) -> Result<(), Panicked> {
        let task_id = admitted.item.meta.id;
        let mailbox_key = admitted.item.mailbox_key.clone();
        let mut progress = Progress::default();
        let ran = catch(|| self.execute(runtime, admitted, worker_id, &mut progress));
        if let Err(panicked) = &ran {
            error!(
                task_id,
                %panicked,
                "panic escaped the executor containment; replacing the worker"
            );
            if !progress.reported {
                let error = TaskError::from(panicked.clone());
                self.events().failed(task_id, &error);
                self.finish(&mailbox_key, Err(error));
                self.ledger.finish(1);
            }
            if !progress.settled {
                self.ledger.settle(1);
            }
            let woken = self.dispatcher.poke(self.now_ms());
            self.launch(woken);
        }
        ran
    }

    /// Run one admitted task on the calling worker under its budget, then
    /// return its permit and hand on whatever that admits.
    ///
    /// The executor call, its future and the budget timer run inside one
    /// [`contain`], so an executor panic — in the call, in a poll, or in the
    /// drop of its future when the budget elapses — becomes
    /// [`TaskError::ExecutorPanicked`] and the task finishes normally.
    /// `progress` records when the completion (or capture) starts and when
    /// the task is settled.
    ///
    /// A task received after the cancel phase began is captured without
    /// running (it never started, so the sink gets its payload). A running
    /// task is raced against the cancel token; when it fires the executor
    /// future is dropped and the task is captured without its payload, which
    /// the executor consumed.
    fn execute(
        &self,
        runtime: &tokio::runtime::Runtime,
        admitted: Admitted<P>,
        worker_id: usize,
        progress: &mut Progress,
    ) {
        let Grant { item: task, permit } = admitted;

        // Negative control S-9 (the 0.2 worker loop): a task received after
        // the shutdown flag is dropped — no result, no capture. Its units and
        // its ledger entry are released so the pool still shuts down; the
        // `shutdown_` accounting tests must find the task missing.
        #[cfg(parking_lot_negative_control = "s-9")]
        if self.shutdown.load(Ordering::SeqCst) {
            drop(task);
            progress.reported = true;
            progress.settled = true;
            let woken = self.dispatcher.complete(permit, self.now_ms());
            self.ledger.settle(1);
            self.launch(woken);
            return;
        }

        if self.cancel.is_cancelled() {
            progress.reported = true;
            progress.settled = true;
            let woken = self.dispatcher.complete(permit, self.now_ms());
            self.capture_unstarted(task);
            self.launch(woken);
            return;
        }

        let WorkerTask {
            payload,
            meta,
            mailbox_key,
        } = task;
        let task_id = meta.id;
        let kept_meta = meta.clone();
        let budget = self.budget(&meta);
        debug!(task_id, ?budget, "worker executing task");
        let events = self.events();
        events.started(task_id, worker_id);
        let started = events.clock();
        // On elapse or cancellation the executor future is dropped, which
        // cancels it at its next await point. The timer is created inside
        // `block_on`: it needs the worker runtime's time driver in context.
        let executor = &self.executor;
        let cancel = &self.cancel;
        let contained = runtime.block_on(contain(async move {
            tokio::select! {
                biased;
                () = cancel.cancelled() => None,
                ran = tokio::time::timeout(budget, executor.execute(payload, meta)) => Some(ran),
            }
        }));
        let outcome = match contained {
            Ok(None) => {
                warn!(task_id, "running task cancelled at the drain deadline");
                progress.reported = true;
                progress.settled = true;
                let woken = self.dispatcher.complete(permit, self.now_ms());
                self.capture(kept_meta, &mailbox_key, None, CancelReason::DrainTimeout);
                self.launch(woken);
                return;
            }
            Ok(Some(Ok(outcome))) => outcome,
            Ok(Some(Err(_elapsed))) => {
                warn!(
                    task_id,
                    ?budget,
                    "task exceeded its budget and was cancelled"
                );
                Err(TaskError::Timeout { budget })
            }
            Err(panicked) => {
                error!(task_id, %panicked, "executor panicked; task failed");
                Err(TaskError::from(panicked))
            }
        };

        // Return the units before publishing the outcome, so a caller that
        // has retrieved the result already sees them released.
        let now_ms = self.now_ms();
        progress.reported = true;
        events.finished(task_id, &outcome, started);
        let woken = self.dispatcher.complete(permit, now_ms);
        self.finish(&mailbox_key, outcome);
        self.ledger.finish(1);
        self.ledger.settle(1);
        progress.settled = true;
        self.launch(woken);
    }

    /// Replace a worker that died: spawn a new thread (with a new runtime)
    /// on the same channel. The replacement takes the dead worker's place in
    /// `live_workers`.
    ///
    /// Workers are replaced until the channel's sender is taken (the end of
    /// a shutdown, or drop), so a worker that dies during a drain is still
    /// replaced. The sender is checked under the `workers` lock, which
    /// `shutdown_with` takes after taking the sender, so a replacement is
    /// either in the list `shutdown_with` reaps or never spawned. A worker
    /// that is not replaced captures the admitted tasks still in the channel,
    /// in case it was the last worker.
    fn respawn(
        self: &Arc<Self>,
        worker_id: usize,
        ready_rx: &Receiver<Admitted<P>>,
        stack_size: usize,
    ) {
        let mut workers = self.workers.lock();
        let winding_down = self.ready_tx.lock().is_none();
        let replaced = if winding_down {
            false
        } else {
            match spawn_worker(worker_id, ready_rx.clone(), Arc::clone(self), stack_size) {
                Ok(handle) => {
                    workers.push(handle);
                    info!(worker_id, "replaced a worker that panicked");
                    true
                }
                Err(spawn_error) => {
                    error!(worker_id, error = %spawn_error, "could not replace a dead worker");
                    false
                }
            }
        };
        drop(workers);
        if !replaced {
            self.worker_exited();
            for Grant { item: task, permit } in ready_rx.try_iter() {
                let woken = self.dispatcher.complete(permit, self.now_ms());
                self.capture_unstarted(task);
                self.launch(woken);
            }
        }
    }

    /// A worker thread stopped serving. `SeqCst`: `shutdown_with` waits for
    /// `live_workers` to reach zero through the ledger (see `crate::ledger`).
    fn worker_exited(&self) {
        self.live_workers.fetch_sub(1, Ordering::SeqCst);
        self.ledger.poke();
    }

    /// Join the worker threads that have exited and detach the rest (a
    /// worker still running a task that ignores cancellation). A replacement
    /// for a worker that died is spawned only while the sender is present
    /// and under the `workers` lock, so none can appear after this.
    fn reap_workers(&self) {
        let workers: Vec<_> = self.workers.lock().drain(..).collect();
        let mut detached = 0_usize;
        for worker in workers {
            if worker.is_finished() {
                if worker.join().is_err() {
                    warn!("a worker thread panicked");
                }
            } else {
                detached += 1;
            }
        }
        if detached > 0 {
            warn!(
                detached,
                "worker threads still running at shutdown were detached; their tasks deliver later"
            );
        }
    }
}

/// Start the thread that offers cancelled tasks to `sink`, one at a time,
/// each bounded by `delta`, and settles them in `ledger`. It runs until every
/// sender is dropped and the channel is empty.
fn spawn_sink_thread<P: Send + 'static>(
    sink: Arc<dyn CancellationSink<P>>,
    delta: Duration,
    ledger: Arc<Ledger>,
) -> std::io::Result<UnboundedSender<CancelledTask<P>>> {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<CancelledTask<P>>();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()?;
    thread::Builder::new()
        .name("pl-cancel-sink".into())
        .spawn(move || {
            runtime.block_on(async move {
                while let Some(cancelled) = rx.recv().await {
                    let task_id = cancelled.meta.id;
                    let answer = offer(&sink, cancelled, delta).await;
                    ledger.end_capture(task_id, answer.as_ref());
                    ledger.settle(1);
                }
            });
        })?;
    Ok(tx)
}

/// The error for a task that will not run because the pool shut down.
fn shutdown_cancelled() -> TaskError {
    TaskError::Cancelled {
        reason: "worker pool shut down".into(),
    }
}

/// Worker pool with dedicated OS threads for CPU/GPU-bound work.
///
/// Each worker thread has its own single-threaded tokio runtime, ensuring
/// that executor work does not block the main async runtime.
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
/// - **No polling**: workers block on channel recv; a result waiter is a
///   suspended future (async) or the caller's own parked thread (blocking),
///   never a blocking-pool thread.
/// - **Result TTL**: a result not retrieved within the result TTL (default:
///   the pool's `default_timeout_ms`) is evicted; see
///   [`with_result_ttl`](Self::with_result_ttl).
/// - **Shutdown**: [`shutdown_with`](Self::shutdown_with) /
///   [`shutdown_with_async`](Self::shutdown_with_async) apply a
///   [`ShutdownPolicy`] and report on every accepted task; tasks cancelled at
///   the drain deadline go to the [`CancellationSink`]
///   ([`with_cancellation_sink`](Self::with_cancellation_sink)). Dropping the
///   pool refuses new work, captures the waiting tasks and lets the workers
///   exit once their current tasks finish, without joining them.
pub struct WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    /// State shared with the workers.
    shared: Arc<Shared<P, R, E>>,

    /// Task ID counter (lock-free atomic).
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
    /// This spawns `config.worker_count` OS threads, each with its own
    /// single-threaded tokio runtime for executing tasks.
    ///
    /// # Errors
    ///
    /// Returns [`SchedulerError::InvalidConfig`] if the configuration is invalid.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "public constructor takes the config by value; changing it is an API change"
    )]
    pub fn new(config: WorkerPoolConfig, executor: E) -> Result<Self, SchedulerError> {
        config.validate().map_err(SchedulerError::invalid_config)?;
        let worker_permits = u32::try_from(config.worker_count).unwrap_or(u32::MAX);

        let (ready_tx, ready_rx) = unbounded::<Admitted<P>>();
        let shared = Arc::new(Shared {
            dispatcher: Dispatcher::in_memory(
                POOL_NAME,
                UnitSemaphore::with_max_permits(config.max_units, worker_permits)
                    .with_resource_limits(config.resource_limits.clone()),
                config.max_queue_depth,
            )
            .with_max_head_skips(config.max_head_skips),
            config: config.clone(),
            results: ResultSlots::new(config.default_timeout()),
            counters: PoolCounters::default(),
            ready_tx: Mutex::new(Some(ready_tx)),
            clock: RwLock::new(Arc::new(SystemClock::new())),
            shutdown: AtomicBool::new(false),
            parked_closed: AtomicBool::new(false),
            cancel: CancellationToken::new(),
            ledger: Arc::new(Ledger::default()),
            sink: RwLock::new(None),
            cancellation: RwLock::new(CancellationConfig::default()),
            capture_tx: Mutex::new(None),
            hooks: HookBus::new(),
            executor,
            workers: Mutex::new(Vec::new()),
            live_workers: AtomicUsize::new(config.worker_count),
        });

        let workers = (0..config.worker_count)
            .map(|worker_id| {
                spawn_worker(
                    worker_id,
                    ready_rx.clone(),
                    Arc::clone(&shared),
                    config.thread_stack_size,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                SchedulerError::internal(format!("failed to spawn worker thread: {error}"))
            })?;
        *shared.workers.lock() = workers;

        info!(
            worker_count = config.worker_count,
            max_units = config.max_units,
            max_queue_depth = config.max_queue_depth,
            "WorkerPool initialized with dedicated OS threads (no-polling design)"
        );

        Ok(Self {
            shared,
            task_id_counter: AtomicU64::new(0),
        })
    }

    /// Read time from `clock` instead of the system clock: deadline checks
    /// at submission and at the head of the queue, and execution budgets.
    ///
    /// Pass a [`TestClock`](prometheus_parking_lot_core::TestClock) clone to
    /// drive deadlines from a test.
    #[must_use]
    pub fn with_clock(self, clock: impl Clock + 'static) -> Self {
        *self.shared.clock.write() = Arc::new(clock);
        self
    }

    /// Keep each result that nobody retrieves for `ttl` after its task
    /// finishes, then evict it (default: the pool's `default_timeout_ms`).
    ///
    /// The TTL is measured on the pool clock's monotonic reading (see
    /// [`with_clock`](Self::with_clock)) and applies to results stored from
    /// now on. A `retrieve` of an evicted result fails once with
    /// [`SchedulerError::ResultExpired`], then with `ResultNotFound`.
    /// Eviction happens on the next pool operation after the TTL, or within
    /// one sweep interval (the TTL, at least 50 ms) on an idle pool.
    #[must_use]
    pub fn with_result_ttl(self, ttl: Duration) -> Self {
        self.shared.results.set_ttl(ttl);
        self
    }

    /// Hand tasks cancelled at shutdown to `sink` (default: a
    /// [`LoggingCancellationSink`], which logs and drops them).
    ///
    /// The sink is called from a dedicated thread, started on the first
    /// cancellation. A running task cancelled at the drain deadline reaches
    /// it without its payload (`None`): the executor consumed it, and
    /// `WorkerPool` payloads need be neither `Clone` nor serializable. Tasks
    /// that never started carry their payload.
    #[must_use]
    pub fn with_cancellation_sink(self, sink: Arc<dyn CancellationSink<P>>) -> Self {
        *self.shared.sink.write() = Some(sink);
        self
    }

    /// Bound each sink call by `config.delta` (default 5 s). The other fields
    /// of [`CancellationConfig`] do not apply to the pool.
    #[must_use]
    pub fn with_cancellation_config(self, config: CancellationConfig) -> Self {
        *self.shared.cancellation.write() = config;
        self
    }

    /// Register a lifecycle hook (cr-31); see the module docs for the events.
    /// Hooks run on the bus's own thread, never on a worker or the
    /// submitting thread.
    #[must_use]
    pub fn with_hook(self, hook: Arc<dyn Hook>) -> Self {
        self.shared.hooks.register(hook);
        self
    }

    /// The pool's hook bus: register more hooks at any time, read its
    /// counters, or [`flush`](HookBus::flush) it.
    #[must_use]
    pub fn hooks(&self) -> &HookBus {
        &self.shared.hooks
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
    /// units and a worker is free; otherwise it waits in the priority queue.
    ///
    /// # Returns
    ///
    /// Returns a `MailboxKey` that can be used to retrieve the result.
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
        // Count the task before reading the flag; SeqCst on both sides (see
        // `crate::ledger`): either this submission is refused or a concurrent
        // `shutdown_with` counts it and waits for it.
        shared.ledger.accept();
        if shared.shutdown.load(Ordering::SeqCst) {
            shared.ledger.settle(1);
            let error = pool_shutdown();
            events.rejected(id, &error);
            return Err(error);
        }
        let now_ms = shared.now_ms();
        if meta.deadline_ms.is_some_and(|deadline| now_ms > deadline) {
            shared.ledger.settle(1);
            let error = SchedulerError::DeadlineExpired { task_id: id };
            events.rejected(id, &error);
            return Err(error);
        }

        // Relaxed: the counter only needs unique values.
        let task_id = self.task_id_counter.fetch_add(1, Ordering::Relaxed);
        let mailbox_key = generate_mailbox_key(task_id);
        shared.results.create(task_id, shared.monotonic());

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
                shared.results.discard(task_id);
                shared.ledger.settle(1);
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
            "task submitted to worker pool"
        );
        shared.launch(admission.dispatched);

        // A shutdown that took the parked queue before this task was parked
        // cannot see it. The park happened inside the dispatch lock, so if
        // the flag reads false here `take_parked` (which takes the lock after
        // the flag is stored) found the task; if it reads true, capture
        // whatever is parked now.
        if shared.parked_closed.load(Ordering::SeqCst) {
            shared.capture_parked();
        }
        Ok(mailbox_key)
    }

    /// Retrieve a result asynchronously, waiting up to `timeout`.
    ///
    /// The wait is a suspended future under a tokio timer: it occupies no
    /// thread, so any number of concurrent callers fit on a runtime whose
    /// blocking pool is exhausted. A result is taken by the first caller; if
    /// the wait times out the result stays in its slot, and a later call can
    /// still take it until the result TTL evicts it. Cancel-safe: dropping
    /// the future leaves the result in place.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::Task`] carrying the [`TaskError`] when the executor
    ///   failed, ran past its budget ([`TaskError::Timeout`]), expired while
    ///   queued or was cancelled at shutdown
    /// - [`SchedulerError::Timeout`] if the result is not available within the
    ///   timeout
    /// - [`SchedulerError::ResultExpired`] if the result was evicted unread
    ///   after the result TTL (reported once)
    /// - [`SchedulerError::ResultNotFound`] if the key was not issued by this
    ///   pool, its result was already taken, or its expiry was already
    ///   reported
    pub async fn retrieve_async(
        &self,
        key: &MailboxKey,
        timeout: Duration,
    ) -> Result<R, SchedulerError> {
        let Some(id) = slot_id(key) else {
            return Err(result_not_found(key));
        };
        let shared = &self.shared;
        match shared
            .results
            .wait(id, timeout, || shared.monotonic())
            .await
        {
            Ok(outcome) => outcome.map_err(SchedulerError::Task),
            Err(missing) => Err(missing_error(key, missing)),
        }
    }

    /// Retrieve a result, blocking the calling thread up to `timeout`.
    ///
    /// For synchronous callers: a plain thread or a `spawn_blocking`
    /// closure. From async code use [`retrieve_async`](Self::retrieve_async).
    ///
    /// # Errors
    ///
    /// As [`retrieve_async`](Self::retrieve_async).
    ///
    /// # Panics
    ///
    /// When called on a thread that is driving async tasks (inside a tokio
    /// runtime's `block_on` or an async task), with tokio's "Cannot block the
    /// current thread from within a runtime" message, instead of parking a
    /// runtime thread.
    #[track_caller]
    pub fn retrieve(&self, key: &MailboxKey, timeout: Duration) -> Result<R, SchedulerError> {
        let Some(id) = slot_id(key) else {
            return Err(result_not_found(key));
        };
        let shared = &self.shared;
        match shared
            .results
            .wait_blocking(id, timeout, || shared.monotonic())
        {
            Ok(outcome) => outcome.map_err(SchedulerError::Task),
            Err(missing) => Err(missing_error(key, missing)),
        }
    }

    /// Skip-ahead counters (cr-20): tasks started ahead of a parked head that
    /// did not fit, and heads the starvation guard reserved capacity for.
    #[must_use]
    pub fn skip_ahead_stats(&self) -> SkipAheadStats {
        self.shared.dispatcher.skip_ahead_stats()
    }

    /// Units held in dimension `kind` by running tasks, or `None` when the
    /// configuration does not limit `kind` (cr-23; `stats().used_units` is
    /// the total over every kind).
    #[must_use]
    pub fn active_units_of(&self, kind: ResourceKind) -> Option<u32> {
        self.shared.dispatcher.units_in_use_of(kind)
    }

    /// Get current pool statistics.
    ///
    /// `worker_count` is the number of worker threads serving the pool: a
    /// worker that dies from a panic is replaced, so it stays at the
    /// configured count until shutdown. `failed_tasks` counts executor
    /// errors and panics, timeouts, expired deadlines and shutdown
    /// cancellations.
    #[must_use]
    pub fn stats(&self) -> PoolStats {
        let shared = &self.shared;
        // Relaxed: a statistic.
        let live_workers = shared.live_workers.load(Ordering::Relaxed);
        let mut stats = shared
            .counters
            .snapshot(live_workers, shared.config.max_units);
        stats.used_units = shared.dispatcher.units_in_use();
        stats.active_tasks = u64::from(shared.dispatcher.running());
        stats.queued_tasks = u64::try_from(shared.dispatcher.parked()).unwrap_or(u64::MAX);
        stats
    }

    /// Shut down the pool as 0.2 did: refuse new submissions, capture the
    /// tasks still waiting, let running tasks finish, and return once the
    /// workers exit or after about 2 s (a worker still running then is
    /// detached and delivers its result later).
    ///
    /// This is [`shutdown_with`](Self::shutdown_with) with
    /// `ShutdownPolicy::Drain { deadline: Duration::ZERO }`. Blocks the
    /// calling thread; safe to call from async code too (the steps run on a
    /// helper thread).
    pub fn shutdown(&self) {
        let _report = self.shutdown_with(ShutdownPolicy::Drain {
            deadline: Duration::ZERO,
        });
    }

    /// [`shutdown_with_async`](Self::shutdown_with_async), blocking the
    /// calling thread until it returns.
    ///
    /// The steps run on a helper thread with its own single-threaded runtime,
    /// so this is safe to call from a plain thread and from inside a runtime
    /// (where it blocks that runtime thread for the duration, like 0.2's
    /// `shutdown`).
    ///
    /// # Panics
    ///
    /// Re-raises a panic from the shutdown steps (which run no user code
    /// except the cancellation sink, whose panics are contained).
    pub fn shutdown_with(&self, policy: ShutdownPolicy) -> ShutdownReport {
        thread::scope(|scope| {
            let helper = scope.spawn(|| {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(self.shutdown_with_async(policy)),
                    Err(build_error) => {
                        error!(error = %build_error, "could not build the shutdown runtime; signalling shutdown only");
                        self.close();
                        self.shared.ledger.report(self.shared.ledger.tally())
                    }
                }
            });
            helper
                .join()
                .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
        })
    }

    /// Shut the pool down under `policy` and report what happened to every
    /// accepted task.
    ///
    /// Submissions are refused at once. Until the policy's drain deadline,
    /// admitted tasks run and parked tasks keep starting as workers and units
    /// free; the drain ends as soon as every accepted task is settled. At the
    /// deadline, parked tasks are cancelled ([`CancelReason::QueueShutdown`]),
    /// and under `DrainThenCancel` and `CancelImmediately` running tasks are
    /// cancelled at their next await point ([`CancelReason::DrainTimeout`])
    /// and any admitted task a worker receives from then on is captured
    /// without running. A cancelled task's result slot holds
    /// [`TaskError::Cancelled`]; the task is offered to the cancellation sink
    /// (bounded by [`CancellationConfig::delta`]), and the call waits at most
    /// `delta` plus one second for that. Finally the workers' channel is
    /// closed and the call waits up to 2 s for the worker threads to exit;
    /// one that does not (an executor blocking without yielding) is detached.
    ///
    /// The report's `unsettled` counts tasks still running at return; their
    /// results still land in their slots.
    pub async fn shutdown_with_async(&self, policy: ShutdownPolicy) -> ShutdownReport {
        let shared = &self.shared;
        let ledger = &shared.ledger;
        let since = ledger.tally();
        // SeqCst: pairs with the `in_flight` increment in `submit` (see
        // `crate::ledger`).
        shared.shutdown.store(true, Ordering::SeqCst);
        info!(?policy, "shutting down worker pool");

        let drained = ledger
            .wait_until(policy.drain_deadline(), |ledger| ledger.in_flight() == 0)
            .await;
        if !drained {
            // SeqCst: pairs with the load in `submit` (see there).
            shared.parked_closed.store(true, Ordering::SeqCst);
            shared.capture_parked();
            if policy.cancels_stragglers() {
                shared.cancel.cancel();
            }
            let bound = shared.cancellation.read().delta + SETTLE_MARGIN;
            if policy.cancels_stragglers() {
                ledger
                    .wait_until(bound, |ledger| ledger.in_flight() == 0)
                    .await;
            } else {
                ledger
                    .wait_until(bound, |ledger| ledger.capturing() == 0)
                    .await;
            }
        }

        // Close the workers' channel: each worker exits once it is empty.
        self.close();
        let exited = ledger
            .wait_until(WORKER_EXIT_BOUND, |_| {
                shared.live_workers.load(Ordering::SeqCst) == 0
            })
            .await;
        if !exited {
            warn!("worker threads did not exit within 2 s of shutdown");
        }
        shared.reap_workers();
        let report = ledger.report(since);
        info!(?report, "worker pool shut down");
        report
    }

    /// Refuse submissions, capture the parked tasks and close the workers'
    /// channel, without waiting (drop, and the fallback of `shutdown_with`).
    fn close(&self) {
        let shared = &self.shared;
        shared.shutdown.store(true, Ordering::SeqCst);
        shared.parked_closed.store(true, Ordering::SeqCst);
        shared.capture_parked();
        drop(shared.ready_tx.lock().take());
    }
}

impl<P, R, E> Drop for WorkerPool<P, R, E>
where
    P: Send + 'static,
    R: Send + 'static,
    E: WorkerExecutor<P, R>,
{
    fn drop(&mut self) {
        // Refuse submissions, capture what is parked and let the workers exit
        // once their channel is empty, without joining them: dropping a pool
        // with tasks still running must not hang. `shutdown_with` is the
        // graceful path.
        self.close();
        debug!("WorkerPool dropped; workers exit once their current tasks finish");
    }
}

/// Spawn a worker thread.
fn spawn_worker<P, R, E>(
    worker_id: usize,
    ready_rx: Receiver<Admitted<P>>,
    shared: Arc<Shared<P, R, E>>,
    stack_size: usize,
) -> std::io::Result<JoinHandle<()>>
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
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(e) => {
                    error!(
                        worker_id = worker_id,
                        error = %e,
                        "Failed to create worker runtime"
                    );
                    shared.worker_exited();
                    return;
                }
            };

            // Worker loop - blocking recv, NO POLLING. An idle worker wakes
            // once per sweep interval to evict expired results. When the
            // sender is dropped (shutdown) and the channel is empty, recv
            // returns Disconnected and the worker exits.
            loop {
                let admitted = match ready_rx.recv_timeout(shared.sweep_interval()) {
                    Ok(admitted) => admitted,
                    Err(RecvTimeoutError::Timeout) => {
                        shared.results.sweep(shared.monotonic());
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                if shared
                    .execute_supervised(&runtime, admitted, worker_id)
                    .is_err()
                {
                    // This thread's runtime unwound mid-task; hand the
                    // channel to a fresh worker and exit.
                    drop(runtime);
                    Shared::respawn(&shared, worker_id, &ready_rx, stack_size);
                    return;
                }
            }

            shared.worker_exited();
            debug!(worker_id = worker_id, "Worker thread exiting");
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use prometheus_parking_lot_core::{ResourceCost, ResourceKind};
    use std::sync::atomic::AtomicUsize;

    /// Test executor that records which thread it runs on.
    #[derive(Clone)]
    struct TestExecutor {
        execution_count: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl WorkerExecutor<String, String> for TestExecutor {
        async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
            self.execution_count.fetch_add(1, Ordering::Relaxed);
            // Simulate some work
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok(format!("Result: {payload}"))
        }
    }

    fn make_meta(id: u64) -> TaskMetadata {
        TaskMetadata {
            id,
            mailbox: None,
            priority: prometheus_parking_lot_core::Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
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
        let key = pool
            .submit_async("hello".to_string(), make_meta(1))
            .await
            .unwrap();

        // Retrieve result
        let result = pool
            .retrieve_async(&key, Duration::from_secs(5))
            .await
            .unwrap();
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
            let key = pool
                .submit_async(format!("task-{i}"), make_meta(i))
                .await
                .unwrap();
            keys.push(key);
        }

        // Retrieve all results
        for (i, key) in keys.iter().enumerate() {
            let result = pool
                .retrieve_async(key, Duration::from_secs(10))
                .await
                .unwrap();
            assert_eq!(result, format!("Result: task-{i}"));
        }

        // Check execution count
        assert_eq!(executor.execution_count.load(Ordering::Relaxed), 10);
    }

    /// Executor that blocks far longer than the shutdown join bound, to prove
    /// `shutdown()` detaches a wedged worker instead of hanging on it (CR-02).
    #[derive(Clone)]
    struct WedgedExecutor;

    #[async_trait]
    impl WorkerExecutor<String, String> for WedgedExecutor {
        async fn execute(
            &self,
            _payload: String,
            _meta: TaskMetadata,
        ) -> Result<String, TaskError> {
            // Much longer than shutdown's 2s-per-worker join bound.
            tokio::time::sleep(Duration::from_secs(30)).await;
            Ok("unreachable".to_string())
        }
    }

    #[test]
    fn test_shutdown_returns_within_bound_with_wedged_worker() {
        let config = WorkerPoolConfig::new()
            .with_worker_count(1)
            .with_max_queue_depth(10);
        let pool = WorkerPool::new(config, WedgedExecutor).unwrap();

        // Occupy the single worker with a 30s task.
        let _ = pool.submit("stuck".to_string(), make_meta(1)).unwrap();
        // Give the worker a moment to pick up the task.
        thread::sleep(Duration::from_millis(100));

        let start = std::time::Instant::now();
        pool.shutdown();
        let elapsed = start.elapsed();

        // Must return within the per-worker bound (2s) plus slack — NOT 30s.
        assert!(
            elapsed < Duration::from_secs(5),
            "shutdown() blocked on a wedged worker: took {elapsed:?}"
        );
    }

    #[test]
    fn test_worker_pool_blocking_api() {
        let executor = TestExecutor {
            execution_count: Arc::new(AtomicUsize::new(0)),
        };

        let config = WorkerPoolConfig::new()
            .with_worker_count(2)
            .with_max_queue_depth(10);

        let pool = WorkerPool::new(config, executor).unwrap();

        // Submit using blocking API
        let key = pool.submit("blocking".to_string(), make_meta(1)).unwrap();

        // Retrieve using blocking API
        let result = pool.retrieve(&key, Duration::from_secs(5)).unwrap();
        assert_eq!(result, "Result: blocking");
    }
}
