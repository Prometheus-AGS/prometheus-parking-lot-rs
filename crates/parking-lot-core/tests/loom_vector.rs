//! S-19: multi-resource admission (cr-23) of the dispatcher + vector
//! semaphore.
//!
//! Run with:
//!
//! ```text
//! RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 \
//!     cargo test -p prometheus-parking-lot-core --test loom_vector --locked
//! ```
//!
//! The pool limits GPU VRAM to 2 units and CPU slots to 1; its total is not
//! binding, so only the per-kind limits can stop an over-admission.
//!
//! Oracle: in no explored schedule do the running jobs hold more than the
//! limit in any dimension (checked by an observer independent of the
//! semaphore); a job short in one dimension waits, and starts once it fits;
//! every accepted job runs exactly once and nothing is left parked.
//!
//! Negative control: with `--cfg parking_lot_negative_control="s-19"` only the
//! first dimension (the packed total and permits) is checked, and the model
//! must report a violation.
#![cfg(parking_lot_loom)]

#[allow(dead_code, reason = "this model uses its own per-dimension oracle")]
mod loom_support;

use loom::sync::Arc;
use loom::sync::atomic::{AtomicU32, Ordering};
use loom::thread;

use loom_support::{Job, Pool};
use prometheus_parking_lot_core::dispatch::Grant;
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{Priority, ResourceKind};

const VRAM: u32 = 2;
const SLOTS: u32 = 1;

fn pool() -> Arc<Pool> {
    Arc::new(Pool::in_memory(
        "loom",
        UnitSemaphore::new(100)
            .with_resource_limit(ResourceKind::GpuVram, VRAM)
            .with_resource_limit(ResourceKind::Cpu, SLOTS),
        8,
    ))
}

/// Per-dimension units held by running jobs, observed independently of the
/// semaphore, and the number of jobs that ran.
#[derive(Debug, Default)]
struct Oracle {
    vram: AtomicU32,
    slots: AtomicU32,
    ran: AtomicU32,
}

/// Run `grants` the way a pool does: each job "executes" (the oracle checks
/// every dimension against its limit), then completes, and whatever its
/// completion admits runs next on this thread.
fn run(pool: &Arc<Pool>, oracle: &Arc<Oracle>, grants: Vec<Grant<Job>>) {
    let mut pending = grants;
    while let Some(grant) = pending.pop() {
        let vram = grant.item.demand.of(ResourceKind::GpuVram);
        let slots = grant.item.demand.of(ResourceKind::Cpu);
        let held_vram = oracle.vram.fetch_add(vram, Ordering::SeqCst) + vram;
        let held_slots = oracle.slots.fetch_add(slots, Ordering::SeqCst) + slots;
        assert!(
            held_vram <= VRAM,
            "capacity violated in GpuVram: {held_vram} units running, limit {VRAM}"
        );
        assert!(
            held_slots <= SLOTS,
            "capacity violated in Cpu: {held_slots} slots running, limit {SLOTS}"
        );
        // A yield inside the critical window lets loom schedule another
        // thread's admission while this job holds its units.
        thread::yield_now();
        oracle.vram.fetch_sub(vram, Ordering::SeqCst);
        oracle.slots.fetch_sub(slots, Ordering::SeqCst);
        oracle.ran.fetch_add(1, Ordering::SeqCst);
        pending.extend(pool.complete(grant.permit, 0).started);
    }
}

/// Three concurrent submitters: two jobs of (1 VRAM, 1 slot), which fit the
/// VRAM together but not the single slot, and one of (2 VRAM, 0 slots), which
/// conflicts with either in VRAM only. No schedule runs more than the limit
/// in any dimension, and all three jobs run.
#[test]
fn loom_vector_no_schedule_exceeds_any_dimension() {
    loom::model(|| {
        let pool = pool();
        let oracle = Arc::new(Oracle::default());

        let submitters: Vec<_> = [
            Job::vector(Priority::Normal, 1, 1),
            Job::vector(Priority::High, 1, 1),
        ]
        .into_iter()
        .map(|job| {
            let pool = Arc::clone(&pool);
            let oracle = Arc::clone(&oracle);
            thread::spawn(move || {
                let admission = pool.submit(job, 0).expect("depth 8 accepts every job");
                run(&pool, &oracle, admission.dispatched.started);
            })
        })
        .collect();

        let admission = pool
            .submit(Job::vector(Priority::Low, 2, 0), 0)
            .expect("depth 8 accepts every job");
        run(&pool, &oracle, admission.dispatched.started);

        for submitter in submitters {
            submitter.join().expect("submitter thread");
        }

        assert_eq!(
            oracle.ran.load(Ordering::SeqCst),
            3,
            "an accepted job was lost"
        );
        assert_eq!(pool.parked(), 0, "lost wakeup: a job is still parked");
        assert_eq!(pool.units_in_use(), 0, "units leaked");
        assert_eq!(pool.units_in_use_of(ResourceKind::GpuVram), Some(0));
        assert_eq!(pool.units_in_use_of(ResourceKind::Cpu), Some(0));
    });
}

/// A job short in one dimension waits: (1 VRAM, 1 slot) runs, and a second
/// (1 VRAM, 1 slot) arrives while one VRAM unit is free but no slot is. It
/// does not start beside the first, whatever the interleaving with the first
/// job's completion; it starts once the slot is returned, and nothing is left
/// parked.
#[test]
fn loom_vector_job_short_in_one_dimension_waits() {
    loom::model(|| {
        let pool = pool();
        let oracle = Arc::new(Oracle::default());

        let first = pool
            .submit(Job::vector(Priority::Normal, 1, 1), 0)
            .expect("accepted");
        assert!(first.started);

        let completer = {
            let pool = Arc::clone(&pool);
            let oracle = Arc::clone(&oracle);
            let grants = first.dispatched.started;
            thread::spawn(move || run(&pool, &oracle, grants))
        };

        let second = pool
            .submit(Job::vector(Priority::High, 1, 1), 0)
            .expect("accepted");
        if second.started {
            // One VRAM unit was free all along; only the returned slot can
            // have admitted it, so the first job had finished.
            assert_eq!(
                oracle.ran.load(Ordering::SeqCst),
                1,
                "started while the only slot was held"
            );
        }
        run(&pool, &oracle, second.dispatched.started);

        completer.join().expect("completer");

        assert_eq!(
            oracle.ran.load(Ordering::SeqCst),
            2,
            "an accepted job was lost"
        );
        assert_eq!(pool.parked(), 0, "lost wakeup: a job is still parked");
        assert_eq!(pool.units_in_use_of(ResourceKind::Cpu), Some(0));
    });
}
