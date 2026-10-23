//! S-20: skip-ahead bin-packing and its starvation bound (cr-20).
//!
//! Run with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-core --locked -E "test(starvation_)"
//! ```
//!
//! The tests drive the real [`Dispatcher`] through a deterministic,
//! virtual-time simulation: every tick, the items whose run ends at that tick
//! complete (through `Dispatcher::complete`, which drains), then the tick's
//! arrivals are submitted (through `Dispatcher::submit`, which parks and
//! drains). Nothing depends on wall-clock time or thread scheduling.
//!
//! Oracle: a large head starts within `max_head_skips` dispatch cycles (one
//! cycle per started item) while small items keep arriving and would fit
//! beside the running work forever; the wall-clock form of the bound (module
//! docs of `dispatch::dispatcher`) holds too.
//!
//! Negative control: with `--cfg parking_lot_negative_control="s-20"` the
//! guard never engages, and `starvation_large_head_starts_within_the_bound`
//! and `starvation_bound_holds_for_a_head_behind_equal_priority_work` must
//! fail: the head starves for the whole horizon.
#![cfg(not(parking_lot_loom))]

use std::collections::BTreeMap;

use prometheus_parking_lot_core::Priority;
use prometheus_parking_lot_core::dispatch::{Dispatcher, Grant, Parkable, Ticket};
use prometheus_parking_lot_core::semaphore::{Permit, UnitSemaphore, UnitVector};

/// A simulated item: who it is, what it costs, how long it runs.
#[derive(Debug)]
struct Job {
    name: &'static str,
    priority: Priority,
    units: u32,
    ticks: u64,
    sequence: u64,
}

impl Parkable for Job {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: self.priority,
            sequence: self.sequence,
            demand: UnitVector::scalar(self.units),
            deadline_ms: None,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.sequence = sequence;
    }
}

const fn job(name: &'static str, priority: Priority, units: u32, ticks: u64) -> Job {
    Job {
        name,
        priority,
        units,
        ticks,
        sequence: 0,
    }
}

/// One start, as the simulation saw it.
#[derive(Debug, Clone, Copy)]
struct Start {
    tick: u64,
    name: &'static str,
}

/// The virtual-time driver around a real dispatcher.
struct Sim {
    dispatcher: Dispatcher<Job>,
    max_units: u32,
    tick: u64,
    /// Running items by end tick: their permits.
    running: BTreeMap<u64, Vec<Permit>>,
    starts: Vec<Start>,
    /// Unit-ticks of work done (for utilization).
    busy: u64,
}

impl Sim {
    fn new(max_units: u32, max_head_skips: u32) -> Self {
        Self {
            dispatcher: Dispatcher::in_memory("sim", UnitSemaphore::new(max_units), usize::MAX)
                .with_max_head_skips(max_head_skips),
            max_units,
            tick: 0,
            running: BTreeMap::new(),
            starts: Vec::new(),
            busy: 0,
        }
    }

    fn launch(&mut self, grants: Vec<Grant<Job>>) {
        for grant in grants {
            let Job { name, ticks, .. } = grant.item;
            self.starts.push(Start {
                tick: self.tick,
                name,
            });
            self.running
                .entry(self.tick + ticks.max(1))
                .or_default()
                .push(grant.permit);
        }
        let in_use = self.dispatcher.units_in_use();
        assert!(
            in_use <= self.max_units,
            "capacity exceeded: {in_use} of {} units",
            self.max_units
        );
    }

    fn submit(&mut self, item: Job) {
        let admission = self.dispatcher.submit(item, 0).expect("unbounded depth");
        self.launch(admission.dispatched.started);
    }

    /// Advance one tick: account the running work, complete what ends now.
    fn step(&mut self) {
        self.busy += u64::from(self.dispatcher.units_in_use());
        self.tick += 1;
        if let Some(permits) = self.running.remove(&self.tick) {
            for permit in permits {
                let dispatched = self.dispatcher.complete(permit, 0);
                self.launch(dispatched.started);
            }
        }
    }

    fn started(&self, name: &str) -> Option<usize> {
        self.starts.iter().position(|start| start.name == name)
    }

    fn utilization(&self) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "small test values")]
        let ratio = self.busy as f64 / (self.tick as f64 * f64::from(self.max_units));
        ratio
    }
}

const HORIZON: u64 = 400;

/// Small one-unit, two-tick items arrive two per tick on four units: they
/// alone keep the pool full. A four-unit high-priority item arrives at tick
/// 10 and is the head at once. It needs every unit, which unbounded
/// skip-ahead never leaves free.
#[test]
fn starvation_large_head_starts_within_the_bound() {
    const MAX_HEAD_SKIPS: u32 = 4;
    let mut sim = Sim::new(4, MAX_HEAD_SKIPS);
    let mut head_parked_at = None;
    let mut running_at_head = 0;
    while sim.tick < HORIZON {
        for _ in 0..2 {
            sim.submit(job("small", Priority::Normal, 1, 2));
        }
        if sim.tick == 10 {
            let before = sim.starts.len();
            sim.submit(job("large", Priority::High, 4, 3));
            assert_eq!(sim.starts.len(), before, "the pool is full; the head waits");
            head_parked_at = Some((sim.starts.len(), sim.tick));
            running_at_head = sim.dispatcher.running();
        }
        sim.step();
    }
    let (starts_before, parked_tick) = head_parked_at.expect("the head arrived");
    let started = sim.started("large").unwrap_or_else(|| {
        panic!(
            "starvation: the head never started in {HORIZON} ticks ({} items skipped ahead)",
            sim.dispatcher.skip_ahead_stats().skipped
        )
    });
    // (1) Dispatch cycles: items started while the head waited.
    let cycles = started - starts_before;
    assert!(
        cycles <= MAX_HEAD_SKIPS as usize,
        "the head started after {cycles} dispatch cycles, bound {MAX_HEAD_SKIPS}"
    );
    // (2) Wall clock: covered by the run times of the items running when the
    // head parked plus at most MAX_HEAD_SKIPS skipped items (2 ticks each).
    let waited = sim.starts[started].tick - parked_tick;
    let bound = (u64::from(running_at_head) + u64::from(MAX_HEAD_SKIPS)) * 2;
    assert!(
        waited <= bound,
        "the head waited {waited} ticks, bound {bound}"
    );
    // The guard engaged and the small items did skip ahead before it.
    let stats = sim.dispatcher.skip_ahead_stats();
    assert!(stats.skipped >= u64::from(MAX_HEAD_SKIPS), "{stats:?}");
    assert!(stats.reservations >= 1, "{stats:?}");
}

/// The large item has the stream's priority, so it first waits behind the
/// small items submitted before it (FIFO). Once it is the head the same bound
/// holds.
#[test]
fn starvation_bound_holds_for_a_head_behind_equal_priority_work() {
    const MAX_HEAD_SKIPS: u32 = 3;
    let mut sim = Sim::new(4, MAX_HEAD_SKIPS);
    while sim.tick < HORIZON {
        for _ in 0..2 {
            sim.submit(job("small", Priority::Normal, 1, 2));
        }
        if sim.tick == 5 {
            for _ in 0..4 {
                sim.submit(job("early", Priority::Normal, 1, 2));
            }
            sim.submit(job("large", Priority::Normal, 3, 4));
        }
        sim.step();
    }
    let started = sim.started("large").unwrap_or_else(|| {
        panic!(
            "starvation: the head never started in {HORIZON} ticks ({} items skipped ahead)",
            sim.dispatcher.skip_ahead_stats().skipped
        )
    });
    // It became the head when the last item parked before it started.
    let became_head = sim
        .starts
        .iter()
        .rposition(|start| start.name == "early")
        .expect("the earlier items started");
    assert!(became_head < started, "FIFO: the earlier items start first");
    let cycles = started - became_head - 1;
    assert!(
        cycles <= MAX_HEAD_SKIPS as usize,
        "the head started after {cycles} dispatch cycles, bound {MAX_HEAD_SKIPS}"
    );
    assert!(sim.dispatcher.skip_ahead_stats().reservations >= 1);
}

/// Strict order wastes the unit a waiting large head leaves free; skip-ahead
/// fills it. Mixed workload on four units: a three-unit, four-tick item every
/// three ticks (more than the pool can serve, so a large item always waits at
/// the head) and a one-unit, one-tick item every tick.
#[test]
fn starvation_small_tasks_skip_ahead_raise_utilization() {
    fn run(max_head_skips: u32) -> Sim {
        let mut sim = Sim::new(4, max_head_skips);
        while sim.tick < 300 {
            if sim.tick.is_multiple_of(3) {
                sim.submit(job("large", Priority::Normal, 3, 4));
            }
            sim.submit(job("small", Priority::Normal, 1, 1));
            sim.step();
        }
        sim
    }
    let strict = run(0);
    let skip = run(prometheus_parking_lot_core::dispatch::DEFAULT_MAX_HEAD_SKIPS);
    assert_eq!(strict.dispatcher.skip_ahead_stats().skipped, 0);
    assert!(skip.dispatcher.skip_ahead_stats().skipped > 0);
    assert!(
        skip.utilization() > strict.utilization(),
        "skip-ahead {:.3} vs strict {:.3}",
        skip.utilization(),
        strict.utilization()
    );
    assert!(skip.starts.len() > strict.starts.len());
}

/// Priority, then FIFO, among the items that fit: with the head blocked, the
/// parked one-unit items skip ahead in priority order, not in arrival order.
#[test]
fn starvation_priority_order_holds_among_fitting_tasks() {
    let mut sim = Sim::new(4, 16);
    sim.submit(job("blocker", Priority::Normal, 3, 5));
    sim.submit(job("head", Priority::Critical, 4, 1));
    // Fits beside the blocker: skips ahead at once and fills the pool.
    sim.submit(job("filler", Priority::Normal, 1, 5));
    for (name, priority) in [
        ("low-a", Priority::Low),
        ("normal-a", Priority::Normal),
        ("high-a", Priority::High),
        ("normal-b", Priority::Normal),
        ("high-b", Priority::High),
    ] {
        sim.submit(job(name, priority, 1, 1));
    }
    while sim.tick < 30 {
        sim.step();
    }
    let order: Vec<_> = sim.starts.iter().map(|start| start.name).collect();
    // Tick 5: the blocker frees 3 units (the head needs 4): high-a, high-b,
    // normal-a skip ahead; the filler frees 1: normal-b. Tick 6: low-a takes
    // the first unit freed. Tick 7: all four units free; the head starts.
    assert_eq!(
        order,
        vec![
            "blocker", "filler", "high-a", "high-b", "normal-a", "normal-b", "low-a", "head"
        ]
    );
}
