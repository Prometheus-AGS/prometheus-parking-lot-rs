//! Shared fixtures for the loom models (`loom_capacity`, `loom_wakeup`).
//!
//! The models drive the real `Dispatcher` and `UnitSemaphore`: under
//! `--cfg parking_lot_loom` their mutex and atomics are loom's, so loom
//! explores the interleavings of the production code.

use loom::sync::Arc;
use loom::sync::atomic::{AtomicU32, Ordering};

use prometheus_parking_lot_core::Priority;
use prometheus_parking_lot_core::dispatch::{Dispatcher, Grant, Parkable, Ticket};

/// A unit of parked work.
#[derive(Debug)]
pub struct Job {
    pub priority: Priority,
    pub units: u32,
    pub sequence: u64,
}

impl Job {
    pub const fn new(priority: Priority, units: u32) -> Self {
        Self {
            priority,
            units,
            sequence: 0,
        }
    }
}

impl Parkable for Job {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: self.priority,
            sequence: self.sequence,
            units: self.units,
            deadline_ms: None,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.sequence = sequence;
    }
}

pub type Pool = Dispatcher<Job>;

/// An observer independent of the semaphore: the units of the jobs actually
/// "running" and the number of jobs that ran.
#[derive(Debug, Default)]
pub struct Oracle {
    held: AtomicU32,
    ran: AtomicU32,
}

impl Oracle {
    pub fn ran(&self) -> u32 {
        self.ran.load(Ordering::SeqCst)
    }
}

/// Run `grants` the way a pool does: each job "executes" (the oracle checks
/// the units held by running jobs against `max_units`), then completes, and
/// whatever its completion admits runs next on this thread.
pub fn run(pool: &Arc<Pool>, oracle: &Arc<Oracle>, max_units: u32, grants: Vec<Grant<Job>>) {
    let mut pending = grants;
    while let Some(grant) = pending.pop() {
        let units = grant.item.units;
        let held = oracle.held.fetch_add(units, Ordering::SeqCst) + units;
        assert!(
            held <= max_units,
            "capacity invariant violated: {held} units running, max_units {max_units}"
        );
        // A yield inside the critical window lets loom schedule another
        // thread's admission while this job holds its units.
        loom::thread::yield_now();
        oracle.held.fetch_sub(units, Ordering::SeqCst);
        oracle.ran.fetch_add(1, Ordering::SeqCst);
        pending.extend(pool.complete(grant.permit, 0).started);
    }
}
