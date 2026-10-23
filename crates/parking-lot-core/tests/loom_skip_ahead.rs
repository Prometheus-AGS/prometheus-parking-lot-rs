//! S-1/S-2/S-20 under skip-ahead (cr-20): the drain now starts items from
//! behind a blocked head, so the admission path has a second shape. This
//! model races completions against a submission that could skip ahead, with
//! a starvation guard whose limit is already reached.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 \
//!     cargo test -p prometheus-parking-lot-core --test loom_skip_ahead --locked
//! ```
//!
//! Oracle: in no explored schedule do the units of running jobs exceed
//! `max_units`; every accepted job runs exactly once; nothing is left parked
//! (no lost wakeup, including for a head the guard reserved capacity for);
//! the head is skipped at most `max_head_skips` times, and the guard engages
//! for it. Under `--cfg parking_lot_negative_control="s-20"` (the guard never
//! engages) the model must fail.
#![cfg(parking_lot_loom)]

mod loom_support;

use loom::sync::Arc;
use loom::thread;

use loom_support::{Job, Oracle, Pool, run};
use prometheus_parking_lot_core::Priority;
use prometheus_parking_lot_core::semaphore::UnitSemaphore;

const MAX_UNITS: u32 = 2;
const MAX_HEAD_SKIPS: u32 = 1;

/// A one-unit job runs; a two-unit high-priority head waits; a one-unit job
/// skips ahead of it, which uses the one skip the guard allows. Then two
/// threads complete the running jobs while a third one-unit job is submitted
/// concurrently. Every schedule reaches a drain that finds the head blocked
/// with its skip spent (the first completion frees one unit, the head needs
/// two; or the submission finds no unit free), so the guard engages in every
/// schedule, and it must keep the late job from skipping a second time.
/// Without the guard (S-20) the late job skips in the schedules where a
/// completion frees a unit first.
#[test]
fn loom_skip_ahead_races_completion_and_keeps_every_invariant() {
    loom::model(|| {
        let pool = Arc::new(
            Pool::in_memory("loom", UnitSemaphore::new(MAX_UNITS), 8)
                .with_max_head_skips(MAX_HEAD_SKIPS),
        );
        let oracle = Arc::new(Oracle::default());

        let first = pool
            .submit(Job::new(Priority::Normal, 1), 0)
            .expect("accepted");
        assert!(first.started);
        let head = pool
            .submit(Job::new(Priority::High, 2), 0)
            .expect("accepted");
        assert!(!head.started, "one unit is free; the head needs two");
        let skipper = pool
            .submit(Job::new(Priority::Low, 1), 0)
            .expect("accepted");
        assert!(skipper.started, "the one skip the guard allows");
        assert_eq!(pool.skip_ahead_stats().skipped, 1);

        let completers: Vec<_> = [first.dispatched.started, skipper.dispatched.started]
            .into_iter()
            .map(|grants| {
                let pool = Arc::clone(&pool);
                let oracle = Arc::clone(&oracle);
                thread::spawn(move || run(&pool, &oracle, MAX_UNITS, grants))
            })
            .collect();
        let late = pool
            .submit(Job::new(Priority::Normal, 1), 0)
            .expect("accepted");
        run(&pool, &oracle, MAX_UNITS, late.dispatched.started);

        for completer in completers {
            completer.join().expect("completer");
        }

        assert_eq!(oracle.ran(), 4, "an accepted job was lost");
        assert_eq!(
            pool.parked(),
            0,
            "lost wakeup: a job is parked while {} of {MAX_UNITS} units are in use",
            pool.units_in_use()
        );
        assert_eq!(pool.units_in_use(), 0, "units leaked");
        let stats = pool.skip_ahead_stats();
        assert!(
            stats.skipped <= u64::from(MAX_HEAD_SKIPS),
            "starvation bound violated: the head was skipped {} times, max_head_skips {MAX_HEAD_SKIPS}",
            stats.skipped
        );
        assert_eq!(
            stats.reservations, 1,
            "the guard did not engage for a head skipped max_head_skips times: {stats:?}"
        );
    });
}
