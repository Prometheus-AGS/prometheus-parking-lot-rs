//! Shutdown accounting shared by the pools (cr-29).
//!
//! Every pool keeps one [`Ledger`]. It counts the tasks the pool has accepted
//! and not yet *settled* — that is, neither reached their own outcome nor been
//! cancelled and handed on (to the cancellation sink or back to the durable
//! queue) — and how every cancellation ended. `shutdown_with` waits on it and
//! turns it into a [`ShutdownReport`].
//!
//! ## Orderings
//!
//! Two store-then-load pairs need a total order, so both sides use `SeqCst`:
//!
//! - **Submission vs shutdown.** A submission increments `in_flight`, then
//!   loads the pool's shutdown flag; `shutdown_with` stores the flag, then
//!   loads `in_flight`. In the single total order of `SeqCst` operations one of
//!   the two loads comes last and sees the other side's write: either the
//!   submission is refused, or the shutdown counts it and waits for it.
//! - **Counter change vs waiter.** A writer changes a counter, then loads
//!   `watching` to decide whether to call `notify_waiters`; a waiter stores
//!   `watching`, registers its `Notified` future (`enable`), then loads the
//!   counter. Either the writer sees `watching` and notifies, or the waiter's
//!   load comes after the writer's change in the total order and sees it. When
//!   the writer notifies, the waiter's `Notified` was created before the
//!   check, so a `notify_waiters` after that creation completes it; the edge
//!   is `Notify`'s own internal `SeqCst` state, which it reads at creation and
//!   bumps on every `notify_waiters`. Gating on `watching` keeps
//!   `notify_waiters` (a mutex acquisition) off the per-task hot path until a
//!   shutdown is actually waiting.
//!
//! The outcome tallies are read only for the report, after `wait_until` has
//! observed the settled state they belong to, so they are `Relaxed`: each is
//! written before the `in_flight`/`capturing` decrement that a waiter
//! acquires (`SeqCst` read-modify-writes are also release operations).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

use crate::cancellation::SinkOutcome;
use crate::shutdown::ShutdownReport;

/// Accepted, settled and cancelled task counts for one pool.
#[derive(Debug, Default)]
pub struct Ledger {
    /// Accepted and not yet settled (durable pool: leased in this process).
    in_flight: AtomicU64,
    /// Pool machinery a shutdown also waits for but that is not a task (the
    /// durable pool's dispatcher while it runs); never reported.
    background: AtomicU64,
    /// Cancelled tasks handed to the sink whose answer is still pending.
    capturing: AtomicU64,
    /// Tasks that reached their own outcome (success, failure, expiry).
    finished: AtomicU64,
    cancelled: AtomicU64,
    recorded: AtomicU64,
    requeued: AtomicU64,
    dropped: AtomicU64,
    unanswered: AtomicU64,
    /// Set once a shutdown waits; gates `notify_waiters`.
    watching: AtomicBool,
    changed: Notify,
}

/// A snapshot of the cumulative tallies, to report the difference later.
#[derive(Debug, Clone, Copy, Default)]
pub struct Tally {
    finished: u64,
    cancelled: u64,
    recorded: u64,
    requeued: u64,
    dropped: u64,
    unanswered: u64,
}

impl Ledger {
    /// Count an accepted task. `SeqCst`: see the module docs (submission vs
    /// shutdown).
    pub fn accept(&self) {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
    }

    /// Settle `n` accepted tasks.
    pub fn settle(&self, n: u64) {
        if n == 0 {
            return;
        }
        self.in_flight.fetch_sub(n, Ordering::SeqCst);
        self.poke();
    }

    /// Count `n` tasks that reached their own outcome. Settling them is the
    /// caller's separate step.
    pub fn finish(&self, n: u64) {
        self.finished.fetch_add(n, Ordering::Relaxed);
    }

    /// A cancelled task is being handed to the sink.
    pub fn begin_capture(&self) {
        self.capturing.fetch_add(1, Ordering::SeqCst);
    }

    /// The sink answered (`Some`), or did not within its bound (`None`).
    ///
    /// An in-memory pool has nothing to requeue into once it shuts down, so a
    /// `Requeue` answer is counted as dropped and logged.
    pub fn end_capture(&self, task_id: u64, answer: Option<&SinkOutcome>) {
        let tally = match answer {
            Some(SinkOutcome::Recorded) => &self.recorded,
            Some(SinkOutcome::Requeue) => {
                tracing::warn!(
                    task_id,
                    "the cancellation sink asked for a requeue, which an in-memory pool \
                     cannot do across a shutdown; record the task in the sink instead"
                );
                &self.dropped
            }
            Some(SinkOutcome::Drop) => &self.dropped,
            None => &self.unanswered,
        };
        tally.fetch_add(1, Ordering::Relaxed);
        self.cancelled.fetch_add(1, Ordering::Relaxed);
        self.capturing.fetch_sub(1, Ordering::SeqCst);
        self.poke();
    }

    /// A durable task was handed back to its queue (nack, requeue, or a lease
    /// left to expire): cancelled at shutdown, too large for this process, or
    /// its outcome could not be stored.
    pub fn requeued(&self) {
        self.requeued.fetch_add(1, Ordering::Relaxed);
        self.cancelled.fetch_add(1, Ordering::Relaxed);
    }

    /// Wake a waiting shutdown after a change it may be waiting for (a worker
    /// exiting, a counter moving). `SeqCst` load: see the module docs.
    pub fn poke(&self) {
        if self.watching.load(Ordering::SeqCst) {
            self.changed.notify_waiters();
        }
    }

    pub fn in_flight(&self) -> u64 {
        self.in_flight.load(Ordering::SeqCst)
    }

    /// Background work still running (see [`Background`]).
    pub fn background(&self) -> u64 {
        self.background.load(Ordering::SeqCst)
    }

    pub fn capturing(&self) -> u64 {
        self.capturing.load(Ordering::SeqCst)
    }

    /// Wait until `done` holds or `bound` elapses; returns whether it holds.
    /// `done` is checked after each change the ledger is poked for.
    pub async fn wait_until(&self, bound: Duration, done: impl Fn(&Self) -> bool) -> bool {
        // SeqCst: see the module docs (counter change vs waiter).
        self.watching.store(true, Ordering::SeqCst);
        let deadline = tokio::time::Instant::now() + bound;
        loop {
            let notified = self.changed.notified();
            let mut notified = std::pin::pin!(notified);
            notified.as_mut().enable();
            if done(self) {
                return true;
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return done(self);
            }
        }
    }

    pub fn tally(&self) -> Tally {
        Tally {
            finished: self.finished.load(Ordering::Relaxed),
            cancelled: self.cancelled.load(Ordering::Relaxed),
            recorded: self.recorded.load(Ordering::Relaxed),
            requeued: self.requeued.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
            unanswered: self.unanswered.load(Ordering::Relaxed),
        }
    }

    /// What happened since `since`, with the tasks still unsettled now.
    pub fn report(&self, since: Tally) -> ShutdownReport {
        let now = self.tally();
        ShutdownReport {
            completed: now.finished - since.finished,
            cancelled: now.cancelled - since.cancelled,
            recorded: now.recorded - since.recorded,
            requeued: now.requeued - since.requeued,
            dropped: now.dropped - since.dropped,
            unanswered: now.unanswered - since.unanswered,
            unsettled: self.in_flight(),
        }
    }
}

/// One accepted task held by the durable pool; settles it when dropped
/// (including by unwinding), so no path out of the pool can leave it counted.
///
/// It is tallied when it settles, after its lease is resolved: as finished
/// once [`finish`](Self::finish) marked it (its outcome is stored), and
/// otherwise as requeued, because every other way out of the pool — a nack,
/// or a lease left to expire — redelivers the task.
pub struct Held {
    ledger: Arc<Ledger>,
    finished: bool,
}

impl Held {
    pub fn new(ledger: &Arc<Ledger>) -> Self {
        ledger.accept();
        Self {
            ledger: Arc::clone(ledger),
            finished: false,
        }
    }

    /// The task reached its own outcome, which is stored, and its lease is
    /// resolved: settle it as finished.
    pub fn finish(mut self) {
        self.finished = true;
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        // The tally first: a waiter that sees the settle sees it.
        if self.finished {
            self.ledger.finish(1);
        } else {
            self.ledger.requeued();
        }
        self.ledger.settle(1);
    }
}

/// Counts pool machinery as background work until dropped (including by
/// unwinding): a shutdown waits for it, but it is not a task, so it is never
/// reported as `unsettled`.
pub struct Background(Arc<Ledger>);

impl Background {
    pub fn new(ledger: &Arc<Ledger>) -> Self {
        // SeqCst: the same store-then-load pairing as `in_flight`.
        ledger.background.fetch_add(1, Ordering::SeqCst);
        Self(Arc::clone(ledger))
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        self.0.background.fetch_sub(1, Ordering::SeqCst);
        self.0.poke();
    }
}

/// Settles one task of `ledger` when dropped (the in-memory pools' running
/// tasks, so the unwinding path settles too).
pub struct SettleOnDrop<'a>(pub &'a Ledger);

impl Drop for SettleOnDrop<'_> {
    fn drop(&mut self) {
        self.0.settle(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A runtime with the timer only (no IO driver), so miri can run these.
    fn timer_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .expect("runtime")
    }

    #[test]
    fn wait_until_sees_a_settle_from_another_thread() {
        timer_runtime().block_on(wait_until_sees_a_settle());
    }

    async fn wait_until_sees_a_settle() {
        let ledger = Arc::new(Ledger::default());
        ledger.accept();
        let settler = Arc::clone(&ledger);
        let handle = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            settler.finish(1);
            settler.settle(1);
        });
        assert!(
            ledger
                .wait_until(Duration::from_secs(5), |l| l.in_flight() == 0)
                .await
        );
        handle.await.expect("settler");
        let report = ledger.report(Tally::default());
        assert_eq!((report.completed, report.unsettled), (1, 0));
    }

    #[test]
    fn wait_until_times_out_and_reports_unsettled() {
        let ledger = Ledger::default();
        ledger.accept();
        let settled = timer_runtime()
            .block_on(ledger.wait_until(Duration::from_millis(20), |l| l.in_flight() == 0));
        assert!(!settled);
        assert_eq!(ledger.report(Tally::default()).unsettled, 1);
    }

    #[test]
    fn captures_are_tallied_by_answer() {
        let ledger = Ledger::default();
        for answer in [
            Some(SinkOutcome::Recorded),
            Some(SinkOutcome::Requeue),
            Some(SinkOutcome::Drop),
            None,
        ] {
            ledger.begin_capture();
            ledger.end_capture(1, answer.as_ref());
        }
        let report = ledger.report(Tally::default());
        assert_eq!(
            (
                report.cancelled,
                report.recorded,
                report.dropped,
                report.unanswered
            ),
            (4, 1, 2, 1)
        );
        assert_eq!(ledger.capturing(), 0);
    }
}
