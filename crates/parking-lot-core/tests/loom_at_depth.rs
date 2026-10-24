//! S-1/S-2/S-20 at `max_queue_depth` (cr-20): a submission that finds the
//! parked queue full is not parked. `Dispatcher::start_at_depth` starts it at
//! once if the drain after a park would have (it precedes the head, or it is
//! the skip candidate, charged to the head through `claim_skip_at_depth`), or
//! rejects it with `QueueFull`. `loom_skip_ahead` uses a depth of 8 and never
//! reaches this path; these models use a depth of 1, so every submission
//! behind the parked head goes through it while a completion races it.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 \
//!     cargo test -p prometheus-parking-lot-core --test loom_at_depth --locked
//! ```
//!
//! Oracle, in every explored schedule:
//!
//! - capacity: the units of running jobs never exceed `max_units` (an
//!   observer independent of the semaphore);
//! - no lost wakeup: every accepted job runs exactly once, nothing is left
//!   parked and no unit is leaked;
//! - starvation bound: the head is skipped at most `max_head_skips` times,
//!   counted both by the dispatcher's stats and independently (a job that
//!   starts at once while the head is still waiting);
//! - priority order among jobs that fit: a low-priority submission never
//!   starts ahead of the high-priority head while the head fits.
//!
//! Negative controls that must make a model fail:
//! `--cfg parking_lot_negative_control="s-1"` (no capacity check),
//! `"s-2"` (park without a drain: lost wakeup), `"s-20"` (the starvation
//! guard never engages), `"s-20-depth"` (only the at-depth path forgets to
//! charge its skip to the head; `loom_skip_ahead` does not catch it) and
//! `"s-20-order"` (the at-depth path skips a head that fits; the priority
//! check of `loom_at_depth_leaves_a_fitting_head_its_capacity` fires).
#![cfg(parking_lot_loom)]

#[allow(dead_code, reason = "this model runs jobs through run_observed")]
mod loom_support;

use loom::sync::Arc;
use loom::sync::atomic::{AtomicBool, Ordering};
use loom::thread;

use loom_support::{Job, Oracle, Pool, run_observed};
use prometheus_parking_lot_core::dispatch::{Admission, Grant};
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{Priority, SchedulerError};

const MAX_UNITS: u32 = 2;
const MAX_QUEUE_DEPTH: usize = 1;
const MAX_HEAD_SKIPS: u32 = 1;

/// The model's fixture: a one-unit job running and a two-unit high-priority
/// head parked, so the queue is at depth and the head does not fit.
struct AtDepth {
    pool: Arc<Pool>,
    oracle: Arc<Oracle>,
    /// Set as the head begins to execute (only the head is `High`).
    head_started: Arc<AtomicBool>,
}

impl AtDepth {
    /// The fixture, and the running one-unit job's grant.
    fn new() -> (Arc<Self>, Vec<Grant<Job>>) {
        let pool = Arc::new(
            Pool::in_memory("loom", UnitSemaphore::new(MAX_UNITS), MAX_QUEUE_DEPTH)
                .with_max_head_skips(MAX_HEAD_SKIPS),
        );
        let first = pool
            .submit(Job::new(Priority::Normal, 1), 0)
            .expect("accepted");
        assert!(first.started);
        let head = pool
            .submit(Job::new(Priority::High, 2), 0)
            .expect("accepted");
        assert!(
            !head.started,
            "capacity invariant violated: the two-unit head started while one of \
             {MAX_UNITS} units is held"
        );
        assert_eq!(pool.parked(), MAX_QUEUE_DEPTH, "the queue is at depth");
        let oracle = Arc::new(Oracle::default());
        oracle.start(1, MAX_UNITS);
        let model = Self {
            pool,
            oracle,
            head_started: Arc::new(AtomicBool::new(false)),
        };
        (Arc::new(model), first.dispatched.started)
    }

    /// The running one-unit job finishes: the oracle counts it as done, then
    /// `release` returns its permit. No yield before the dispatch lock in
    /// either pass (see `loom_support::model`): this completion must be able
    /// to land between the two submissions on the main thread.
    fn finish_first(&self, first: Vec<Grant<Job>>, release: impl FnOnce(Grant<Job>)) {
        let [grant] = <[Grant<Job>; 1]>::try_from(first).expect("one running job");
        self.oracle.finish(grant.item.units);
        release(grant);
    }

    /// Run `grants` on this thread, recording when the head starts.
    fn run(&self, grants: Vec<Grant<Job>>) {
        let head_started = Arc::clone(&self.head_started);
        run_observed(&self.pool, &self.oracle, MAX_UNITS, grants, move |job| {
            if job.priority == Priority::High {
                head_started.store(true, Ordering::SeqCst);
            }
        });
    }

    fn head_started(&self) -> bool {
        self.head_started.load(Ordering::SeqCst)
    }

    /// Submit a one-unit low-priority job behind the head.
    ///
    /// `head_fits` says whether the head fitted the free capacity just
    /// before the call. Returns the outcome, with the grants to run.
    fn submit_low(&self, head_fits: bool) -> Outcome {
        let admission = match self.pool.submit(Job::new(Priority::Low, 1), 0) {
            Ok(admission) => admission,
            Err(SchedulerError::QueueFull { .. }) => return Outcome::rejected(),
            Err(other) => panic!("unexpected error: {other}"),
        };
        let Admission {
            started,
            dispatched,
            ..
        } = admission;
        // Started at once while the head waits: a skip. It holds a unit until
        // this thread runs it, so the two-unit head cannot start before this
        // read; a start after the head ran (the queue was empty) is not one.
        // Happens-before for that case: the head's runner stores the flag
        // (SeqCst) before it completes the head, the completion's unlock of
        // the dispatch lock synchronizes-with our submit's lock, so this load
        // reads `true`.
        let skip = started && !self.head_started();
        if head_fits {
            assert!(
                !skip,
                "priority order violated: a Low job started ahead of the High head \
                 while the head fit the free capacity"
            );
        }
        Outcome {
            accepted: true,
            skip,
            grants: dispatched.started,
        }
    }

    /// The invariants every schedule must end in.
    fn check(&self, outcomes: &[Outcome]) {
        let accepted = outcomes.iter().filter(|outcome| outcome.accepted).count();
        let ran = usize::try_from(self.oracle.ran()).expect("small");
        // The fixture's running job and its head, and every accepted one.
        let expected = 2 + accepted;
        assert_eq!(
            ran, expected,
            "an accepted job was lost: {ran} of {expected} ran"
        );
        assert_eq!(
            self.pool.parked(),
            0,
            "lost wakeup: a job is parked while {} of {MAX_UNITS} units are in use",
            self.pool.units_in_use()
        );
        assert_eq!(self.pool.units_in_use(), 0, "units leaked");
        let observed = outcomes.iter().filter(|outcome| outcome.skip).count();
        assert!(
            observed <= MAX_HEAD_SKIPS as usize,
            "starvation bound violated: {observed} jobs started at depth while the \
             head waited, max_head_skips {MAX_HEAD_SKIPS}"
        );
        let stats = self.pool.skip_ahead_stats();
        assert!(
            stats.skipped <= u64::from(MAX_HEAD_SKIPS),
            "starvation bound violated: the head was skipped {} times, max_head_skips \
             {MAX_HEAD_SKIPS}",
            stats.skipped
        );
    }
}

/// One low-priority submission's result.
struct Outcome {
    accepted: bool,
    skip: bool,
    grants: Vec<Grant<Job>>,
}

impl Outcome {
    const fn rejected() -> Self {
        Self {
            accepted: false,
            skip: false,
            grants: Vec::new(),
        }
    }
}

/// Two submissions at depth race the completion of the running job. Before
/// the completion, the first is the one skip the guard allows and the second
/// is rejected; after it, the head starts and the submissions park behind it
/// or start once it has run. A completion between the two submissions frees
/// a unit the head still cannot use, and the second submission must not take
/// it as a second skip.
#[test]
fn loom_at_depth_skip_races_completion_and_keeps_every_invariant() {
    loom_support::model(|| {
        let (model, first) = AtDepth::new();
        let completer = {
            let model = Arc::clone(&model);
            thread::spawn(move || {
                model.finish_first(first, |grant| {
                    let dispatched = model.pool.complete(grant.permit, 0);
                    model.run(dispatched.started);
                });
            })
        };
        // The head never fits outside the dispatch lock here: `complete`
        // starts it in the same critical section that frees its units.
        let outcomes = [model.submit_low(false), model.submit_low(false)];
        let outcomes = outcomes.map(|mut outcome| {
            model.run(std::mem::take(&mut outcome.grants));
            outcome
        });
        completer.join().expect("completer");
        model.check(&outcomes);
    });
}

/// The running job's permit is dropped outside the dispatch lock (as when a
/// task unwinds) and the owner pokes the dispatcher afterwards. Between the
/// drop and the poke the head fits but is still parked: a submission at
/// depth must leave the capacity to it (`QueueFull`), not skip it. The same
/// invariants hold as when the completion drains.
#[test]
fn loom_at_depth_leaves_a_fitting_head_its_capacity() {
    loom_support::model(|| {
        let (model, first) = AtDepth::new();
        let released = Arc::new(AtomicBool::new(false));
        let completer = {
            let model = Arc::clone(&model);
            let released = Arc::clone(&released);
            thread::spawn(move || {
                model.finish_first(first, drop);
                // The release on the semaphore is sequenced before this store;
                // a submitter that loads `true` sees the units returned.
                released.store(true, Ordering::SeqCst);
                let dispatched = model.pool.poke(0);
                model.run(dispatched.started);
            })
        };
        // The head fits exactly when the running job's unit is back, no
        // skip of ours holds a unit, and the head has not started.
        let fits = |held: bool| released.load(Ordering::SeqCst) && !held && !model.head_started();
        let a = model.submit_low(fits(false));
        let held = a.skip;
        let b = model.submit_low(fits(held));
        let outcomes = [a, b].map(|mut outcome| {
            model.run(std::mem::take(&mut outcome.grants));
            outcome
        });
        completer.join().expect("completer");
        model.check(&outcomes);
    });
}
