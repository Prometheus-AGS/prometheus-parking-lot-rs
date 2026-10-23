//! S-1/S-2 under skip-ahead (cr-20): the drain now starts items from behind a
//! blocked head, so the admission path has a second shape. This model races
//! completions against submissions that can skip ahead, with a starvation
//! guard small enough to engage.
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
//! (no lost wakeup, including for a head the guard reserved capacity for).
#![cfg(parking_lot_loom)]

mod loom_support;

use loom::sync::Arc;
use loom::thread;

use loom_support::{Job, Oracle, Pool, run};
use prometheus_parking_lot_core::Priority;
use prometheus_parking_lot_core::semaphore::UnitSemaphore;

const MAX_UNITS: u32 = 2;

/// A one-unit job runs; a two-unit high-priority head waits. One thread
/// completes the running job while two submitters race in one-unit jobs that
/// can skip ahead of the head (the guard allows one skip). Whatever the
/// interleaving, capacity holds, all four jobs run and nothing stays parked.
#[test]
fn loom_skip_ahead_races_completion_and_keeps_every_invariant() {
    loom::model(|| {
        let pool = Arc::new(
            Pool::in_memory("loom", UnitSemaphore::new(MAX_UNITS), 8).with_max_head_skips(1),
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

        let completer = {
            let pool = Arc::clone(&pool);
            let oracle = Arc::clone(&oracle);
            let grants = first.dispatched.started;
            thread::spawn(move || run(&pool, &oracle, MAX_UNITS, grants))
        };
        let submitter = {
            let pool = Arc::clone(&pool);
            let oracle = Arc::clone(&oracle);
            thread::spawn(move || {
                let admission = pool
                    .submit(Job::new(Priority::Low, 1), 0)
                    .expect("accepted");
                run(&pool, &oracle, MAX_UNITS, admission.dispatched.started);
            })
        };
        let admission = pool
            .submit(Job::new(Priority::Normal, 1), 0)
            .expect("accepted");
        run(&pool, &oracle, MAX_UNITS, admission.dispatched.started);

        completer.join().expect("completer");
        submitter.join().expect("submitter");

        assert_eq!(oracle.ran(), 4, "an accepted job was lost");
        assert_eq!(
            pool.parked(),
            0,
            "lost wakeup: a job is parked while {} of {MAX_UNITS} units are in use",
            pool.units_in_use()
        );
        assert_eq!(pool.units_in_use(), 0, "units leaked");
        let stats = pool.skip_ahead_stats();
        assert!(stats.skipped <= 2, "{stats:?}");
    });
}
