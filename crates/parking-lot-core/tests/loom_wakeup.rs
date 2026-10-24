//! S-2: no lost wakeup under submit/complete races.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 \
//!     cargo test -p prometheus-parking-lot-core --test loom_wakeup --locked
//! ```
//!
//! Oracle: every accepted job is dispatched; no job stays parked while
//! capacity sufficient for it is free; the depth limit is exact.
//!
//! Negative control: with `--cfg parking_lot_negative_control="s-2"` the
//! dispatcher takes the admission decision and the enqueue in separate
//! critical sections (the shape of `main@90a6194`), and the model must find the
//! stranded-job schedule.
#![cfg(parking_lot_loom)]

mod loom_support;

use loom::sync::Arc;
use loom::thread;

use loom_support::{Job, Oracle, Pool, run};
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{Priority, SchedulerError};

/// A completion races a submission that cannot start yet: whichever order
/// the dispatch lock admits them in, the submitted job runs.
#[test]
fn loom_wakeup_completion_racing_a_park_is_not_lost() {
    loom_support::model(|| {
        let pool = Arc::new(Pool::in_memory("loom", UnitSemaphore::new(1), 4));
        let oracle = Arc::new(Oracle::default());

        // Job A holds the only unit.
        let first = pool
            .submit(Job::new(Priority::Normal, 1), 0)
            .expect("accepted");
        assert!(first.started);

        let completer = {
            let pool = Arc::clone(&pool);
            let oracle = Arc::clone(&oracle);
            let grants = first.dispatched.started;
            thread::spawn(move || run(&pool, &oracle, 1, grants))
        };
        let submitter = {
            let pool = Arc::clone(&pool);
            let oracle = Arc::clone(&oracle);
            thread::spawn(move || {
                let admission = pool
                    .submit(Job::new(Priority::Normal, 1), 0)
                    .expect("accepted");
                run(&pool, &oracle, 1, admission.dispatched.started);
            })
        };
        completer.join().expect("completer");
        submitter.join().expect("submitter");

        assert_eq!(
            pool.parked(),
            0,
            "lost wakeup: a job is parked while {} of 1 units are in use",
            pool.units_in_use()
        );
        assert_eq!(oracle.ran(), 2, "an accepted job never ran");
        assert_eq!(pool.units_in_use(), 0);
    });
}

/// Two submitters race for the last parking space while capacity is held:
/// exactly one is parked and one is rejected, and the parked one runs when
/// capacity frees.
#[test]
fn loom_wakeup_depth_limit_is_exact() {
    loom_support::model(|| {
        let pool = Arc::new(Pool::in_memory("loom", UnitSemaphore::new(1), 1));
        let oracle = Arc::new(Oracle::default());
        let first = pool
            .submit(Job::new(Priority::Normal, 1), 0)
            .expect("accepted");

        let submitters: Vec<_> = (0..2)
            .map(|_| {
                let pool = Arc::clone(&pool);
                thread::spawn(
                    move || match pool.submit(Job::new(Priority::Normal, 1), 0) {
                        Ok(admission) => {
                            assert!(!admission.started, "capacity is held; nothing can start");
                            true
                        }
                        Err(SchedulerError::QueueFull { .. }) => false,
                        Err(other) => panic!("unexpected error: {other}"),
                    },
                )
            })
            .collect();
        let accepted = submitters
            .into_iter()
            .map(|submitter| submitter.join().expect("submitter"))
            .filter(|&accepted| accepted)
            .count();

        assert_eq!(accepted, 1, "depth limit 1 admitted {accepted} parked jobs");
        assert_eq!(pool.parked(), 1);
        run(&pool, &oracle, 1, first.dispatched.started);
        assert_eq!(oracle.ran(), 2);
        assert_eq!(pool.parked(), 0);
    });
}
