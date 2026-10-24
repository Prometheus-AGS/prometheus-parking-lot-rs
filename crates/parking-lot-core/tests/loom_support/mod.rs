//! Shared fixtures for the loom models (`loom_capacity`, `loom_wakeup`,
//! `loom_skip_ahead`, `loom_at_depth`, `loom_vector`).
//!
//! The models drive the real `Dispatcher` and `UnitSemaphore`: under
//! `--cfg parking_lot_loom` their mutex and atomics are loom's, so loom
//! explores the interleavings of the production code.

use loom::sync::Arc;
use loom::sync::atomic::{AtomicU32, Ordering};

use prometheus_parking_lot_core::dispatch::{Dispatcher, Grant, Parkable, Ticket};
use prometheus_parking_lot_core::semaphore::UnitVector;
use prometheus_parking_lot_core::{Priority, ResourceCost, ResourceKind};

/// A unit of parked work.
#[derive(Debug)]
pub struct Job {
    pub priority: Priority,
    pub units: u32,
    pub demand: UnitVector,
    pub sequence: u64,
}

impl Job {
    /// A single-dimension job of `units` units.
    pub const fn new(priority: Priority, units: u32) -> Self {
        Self {
            priority,
            units,
            demand: UnitVector::scalar(units),
            sequence: 0,
        }
    }

    /// A job needing `vram` GPU VRAM units and `slots` CPU slots (cr-23).
    #[allow(dead_code, reason = "used by the loom_vector model only")]
    pub fn vector(priority: Priority, vram: u32, slots: u32) -> Self {
        let cost = ResourceCost::new(ResourceKind::GpuVram, vram).with(ResourceKind::Cpu, slots);
        let demand = UnitVector::from_cost(&cost);
        Self {
            priority,
            units: demand.total(),
            demand,
            sequence: 0,
        }
    }
}

impl Parkable for Job {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: self.priority,
            sequence: self.sequence,
            demand: self.demand,
            deadline_ms: None,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.sequence = sequence;
    }
}

pub type Pool = Dispatcher<Job>;

std::thread_local! {
    /// Whether [`window_yield`] yields in the current [`model`] pass.
    static YIELD_IN_WINDOW: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

/// Check `f` with `loom::model` twice: once with a `yield_now` inside each
/// running job's window ([`window_yield`]) and once without.
///
/// Within the preemption bound the two passes reach different schedules.
/// Loom deprioritizes a thread that yields until another thread has run, and
/// DPOR adds no preemption to bring it back while its next step conflicts
/// with nothing, so with the yield a completion is never scheduled between
/// two dispatch-lock sections of a thread that keeps running (a submission
/// racing a completion). Without it, the running job's later completions are
/// no longer interleaved with the other threads' submissions for free. Each
/// pass alone missed lock orders the other reached.
///
/// The switch is thread-local: loom runs a model's threads as generators on
/// the calling OS thread, and libtest runs tests in parallel on separate ones.
pub fn model<F>(f: F)
where
    F: Fn() + Sync + Send + 'static,
{
    let f = std::sync::Arc::new(f);
    for yields in [true, false] {
        YIELD_IN_WINDOW.with(|cell| cell.set(yields));
        eprintln!("loom pass: yield in the running window = {yields}");
        let f = std::sync::Arc::clone(&f);
        loom::model(move || f());
    }
}

/// The yield inside a running job's window, in the pass that has it (see
/// [`model`]): it lets loom schedule another thread's admission while the job
/// holds its units.
pub fn window_yield() {
    if YIELD_IN_WINDOW.with(std::cell::Cell::get) {
        loom::thread::yield_now();
    }
}

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

    /// A job of `units` units begins to execute: check the units held by
    /// running jobs against `max_units`.
    pub fn start(&self, units: u32, max_units: u32) {
        let held = self.held.fetch_add(units, Ordering::SeqCst) + units;
        assert!(
            held <= max_units,
            "capacity invariant violated: {held} units running, max_units {max_units}"
        );
    }

    /// A job of `units` units has finished executing (before its permit is
    /// returned).
    pub fn finish(&self, units: u32) {
        self.held.fetch_sub(units, Ordering::SeqCst);
        self.ran.fetch_add(1, Ordering::SeqCst);
    }
}

/// Run `grants` the way a pool does: each job "executes" (the oracle checks
/// the units held by running jobs against `max_units`), then completes, and
/// whatever its completion admits runs next on this thread.
pub fn run(pool: &Arc<Pool>, oracle: &Arc<Oracle>, max_units: u32, grants: Vec<Grant<Job>>) {
    run_observed(pool, oracle, max_units, grants, |_| {});
}

/// [`run`], calling `on_start(job)` as each job begins to execute, before it
/// completes.
pub fn run_observed(
    pool: &Arc<Pool>,
    oracle: &Arc<Oracle>,
    max_units: u32,
    grants: Vec<Grant<Job>>,
    on_start: impl Fn(&Job),
) {
    let mut pending = grants;
    while let Some(grant) = pending.pop() {
        on_start(&grant.item);
        let units = grant.item.units;
        oracle.start(units, max_units);
        window_yield();
        oracle.finish(units);
        pending.extend(pool.complete(grant.permit, 0).started);
    }
}
