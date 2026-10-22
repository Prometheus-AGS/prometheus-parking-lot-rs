//! S-1: capacity invariant of the dispatcher + unit-weighted semaphore.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 \
//!     cargo test -p prometheus-parking-lot-core --test loom_capacity --locked
//! ```
//!
//! Oracle: in no explored schedule do the units of running jobs exceed
//! `max_units`, and every accepted job runs exactly once.
//!
//! Negative control: with `--cfg parking_lot_negative_control="s-1"` the
//! semaphore's admission check is skipped and the model must fail.
#![cfg(parking_lot_loom)]

mod loom_support;

use loom::sync::Arc;
use loom::thread;

use loom_support::{Job, Oracle, Pool, run};
use prometheus_parking_lot_core::Priority;
use prometheus_parking_lot_core::semaphore::UnitSemaphore;

const MAX_UNITS: u32 = 2;

/// Three concurrent submitters whose combined cost (2 + 1 + 1) exceeds the
/// two units: at most two units ever run, and all three jobs run.
#[test]
fn loom_capacity_never_exceeded_and_no_job_lost() {
    loom::model(|| {
        let pool = Arc::new(Pool::in_memory("loom", UnitSemaphore::new(MAX_UNITS), 8));
        let oracle = Arc::new(Oracle::default());

        let submitters: Vec<_> = [(Priority::Normal, 2), (Priority::High, 1)]
            .into_iter()
            .map(|(priority, units)| {
                let pool = Arc::clone(&pool);
                let oracle = Arc::clone(&oracle);
                thread::spawn(move || {
                    let admission = pool
                        .submit(Job::new(priority, units), 0)
                        .expect("depth 8 accepts every job");
                    run(&pool, &oracle, MAX_UNITS, admission.dispatched.started);
                })
            })
            .collect();

        let admission = pool
            .submit(Job::new(Priority::Low, 1), 0)
            .expect("depth 8 accepts every job");
        run(&pool, &oracle, MAX_UNITS, admission.dispatched.started);

        for submitter in submitters {
            submitter.join().expect("submitter thread");
        }

        assert_eq!(oracle.ran(), 3, "an accepted job was lost");
        assert_eq!(pool.parked(), 0, "a job is still parked");
        assert_eq!(pool.units_in_use(), 0, "units leaked");
    });
}

/// A worker-pool style semaphore that also bounds holders: with two units and
/// one permit, two one-unit jobs never run together.
#[test]
fn loom_capacity_permit_bound_holds() {
    loom::model(|| {
        let pool = Arc::new(Pool::in_memory(
            "loom",
            UnitSemaphore::with_max_permits(MAX_UNITS, 1),
            8,
        ));
        let running = Arc::new(loom::sync::atomic::AtomicU32::new(0));

        let spawn_submitter = || {
            let pool = Arc::clone(&pool);
            let running = Arc::clone(&running);
            thread::spawn(move || {
                let admission = pool
                    .submit(Job::new(Priority::Normal, 1), 0)
                    .expect("depth 8 accepts every job");
                let mut pending = admission.dispatched.started;
                while let Some(grant) = pending.pop() {
                    let now = running.fetch_add(1, loom::sync::atomic::Ordering::SeqCst) + 1;
                    assert!(now <= 1, "permit bound violated: {now} jobs running");
                    thread::yield_now();
                    running.fetch_sub(1, loom::sync::atomic::Ordering::SeqCst);
                    pending.extend(pool.complete(grant.permit, 0).started);
                }
            })
        };
        let a = spawn_submitter();
        let b = spawn_submitter();
        a.join().expect("submitter a");
        b.join().expect("submitter b");
        assert_eq!(pool.parked(), 0);
        assert_eq!(pool.running(), 0);
    });
}
