//! Utilization (cr-40, extends the cr-20 skip-ahead benchmark): units busy
//! over time under mixed workloads, strict order against skip-ahead, one
//! dimension against a cost vector with per-kind limits.
//!
//! A deterministic, virtual-time simulation drives the real core `Dispatcher`
//! and `UnitSemaphore`: every tick, items whose run time is over complete
//! (returning their permits, which starts parked work), then the tick's
//! arrivals are submitted. Time is virtual, so the numbers depend only on the
//! scheduling decisions, not on the machine: the same seed gives the same
//! numbers everywhere. Criterion then times one simulation run (the
//! dispatcher's own cost, not the workload's).
//!
//! Workloads, 2,000 ticks each, fixed seed `0x5eed`:
//!
//! - `mixed` (one dimension, 16 units): a 12-unit, 8-tick item every 9 ticks
//!   and, every tick, 0 to 3 small items of 1 or 2 units running 1 to 3 ticks;
//!   about 95% of capacity offered. A large item regularly waits at the head
//!   for 12 free units.
//! - `overload` (one dimension, 16 units): as `mixed` with 0 to 5 small items
//!   per tick, about 115% offered. Without the guard a large head can wait
//!   indefinitely.
//! - `multi` (cost vectors, 16 units in total, at most 8 `GpuVram` and 12
//!   `Cpu`): a large item of 6 `GpuVram` + 2 `Cpu` for 8 ticks every 9 ticks
//!   and, every tick, 0 to 3 small items, each either 1 or 2 `Cpu` or 1 or 2
//!   `GpuVram` units for 1 to 3 ticks. `GpuVram` is the bottleneck (about 95%
//!   of its limit offered), `Cpu` is lightly loaded (about a third).
//!
//! Per workload and skip-ahead setting the report gives utilization (busy
//! unit-ticks over capacity times ticks, and per limited dimension for
//! `multi`), the spread of utilization over 100-tick windows (units busy over
//! time), items started, and the wait of the large items (ticks from
//! submission to start, p50 and max). Printed, and written to
//! `target/bench-report/utilization.json`.
//!
//! ```text
//! cargo bench --all-features --bench utilization
//! ```

#[allow(
    dead_code,
    reason = "each suite uses a different part of the shared module"
)]
mod support;

use std::collections::BTreeMap;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use serde_json::{Value, json};

use prometheus_parking_lot_core::dispatch::{
    DEFAULT_MAX_HEAD_SKIPS, Dispatcher, Grant, Parkable, Ticket,
};
use prometheus_parking_lot_core::semaphore::{Permit, UnitSemaphore, UnitVector};
use prometheus_parking_lot_core::{Priority, ResourceCost, ResourceKind};

use support::{Lcg, quantile, ratio};

const MAX_UNITS: u32 = 16;
const TICKS: u64 = 2_000;
const WINDOW: u64 = 100;
const SEED: u64 = 0x5eed;
/// Per-kind limits of the `multi` workload.
const GPU_LIMIT: u32 = 8;
const CPU_LIMIT: u32 = 12;

#[derive(Debug)]
struct Job {
    large: bool,
    demand: UnitVector,
    ticks: u64,
    submitted: u64,
    sequence: u64,
}

impl Parkable for Job {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: Priority::Normal,
            sequence: self.sequence,
            demand: self.demand,
            deadline_ms: None,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.sequence = sequence;
    }
}

/// One arrival: large or small, demand and run time in ticks.
type Arrival = (bool, UnitVector, u64);

/// A workload: the semaphore it runs on and its arrivals per tick.
#[derive(Debug, Clone, Copy)]
enum Workload {
    /// One dimension; the most small items arriving per tick.
    Scalar(&'static str, u64),
    /// Cost vectors with per-kind limits.
    Multi,
}

impl Workload {
    const ALL: [Self; 3] = [
        Self::Scalar("mixed", 3),
        Self::Scalar("overload", 5),
        Self::Multi,
    ];

    const fn name(self) -> &'static str {
        match self {
            Self::Scalar(name, _) => name,
            Self::Multi => "multi",
        }
    }

    fn semaphore(self) -> UnitSemaphore {
        match self {
            Self::Scalar(..) => UnitSemaphore::new(MAX_UNITS),
            Self::Multi => UnitSemaphore::new(MAX_UNITS)
                .with_resource_limit(ResourceKind::GpuVram, GPU_LIMIT)
                .with_resource_limit(ResourceKind::Cpu, CPU_LIMIT),
        }
    }

    /// The arrivals of `tick`. The scalar sequence is exactly the cr-20
    /// benchmark's, so its numbers stay comparable.
    fn arrivals(self, tick: u64, rng: &mut Lcg) -> Vec<Arrival> {
        let units = |rng: &mut Lcg| u32::try_from(1 + rng.next(2)).unwrap_or(1);
        let mut arrivals = Vec::new();
        match self {
            Self::Scalar(_, small_per_tick) => {
                if tick.is_multiple_of(9) {
                    arrivals.push((true, UnitVector::scalar(12), 8));
                }
                for _ in 0..rng.next(small_per_tick + 1) {
                    let demand = UnitVector::scalar(units(rng));
                    arrivals.push((false, demand, 1 + rng.next(3)));
                }
            }
            Self::Multi => {
                if tick.is_multiple_of(9) {
                    let cost =
                        ResourceCost::new(ResourceKind::GpuVram, 6).with(ResourceKind::Cpu, 2);
                    arrivals.push((true, UnitVector::from_cost(&cost), 8));
                }
                for _ in 0..rng.next(4) {
                    let kind = if rng.next(2) == 0 {
                        ResourceKind::Cpu
                    } else {
                        ResourceKind::GpuVram
                    };
                    let demand = UnitVector::from_cost(&ResourceCost::new(kind, units(rng)));
                    arrivals.push((false, demand, 1 + rng.next(3)));
                }
            }
        }
        arrivals
    }
}

/// Outcome of one simulated run.
#[derive(Debug, Default)]
struct Report {
    busy_unit_ticks: u64,
    busy_gpu_ticks: u64,
    busy_cpu_ticks: u64,
    /// Busy units summed per `WINDOW` ticks.
    windows: Vec<u64>,
    started: u64,
    large_waits: Vec<u64>,
}

impl Report {
    fn utilization(&self) -> f64 {
        ratio(self.busy_unit_ticks, TICKS * u64::from(MAX_UNITS))
    }

    fn gpu_utilization(&self) -> f64 {
        ratio(self.busy_gpu_ticks, TICKS * u64::from(GPU_LIMIT))
    }

    fn cpu_utilization(&self) -> f64 {
        ratio(self.busy_cpu_ticks, TICKS * u64::from(CPU_LIMIT))
    }

    fn window_utilization(&self) -> Vec<f64> {
        self.windows
            .iter()
            .map(|busy| ratio(*busy, WINDOW * u64::from(MAX_UNITS)))
            .collect()
    }

    fn large_wait(&self, q: f64) -> u64 {
        let mut waits = self.large_waits.clone();
        waits.sort_unstable();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "index into a short vector"
        )]
        let index = ((waits.len().saturating_sub(1)) as f64 * q).round() as usize;
        waits.get(index).copied().unwrap_or(0)
    }

    fn json(&self, workload: Workload) -> Value {
        let windows = self.window_utilization();
        let mut value = json!({
            "utilization": round3(self.utilization()),
            "window_utilization": {
                "min": round3(quantile(&windows, 0.0)),
                "p50": round3(quantile(&windows, 0.5)),
                "max": round3(quantile(&windows, 1.0)),
            },
            "started": self.started,
            "large_started": self.large_waits.len(),
            "large_wait_ticks": { "p50": self.large_wait(0.5), "max": self.large_wait(1.0) },
        });
        if matches!(workload, Workload::Multi) {
            value["utilization_gpu_vram"] = json!(round3(self.gpu_utilization()));
            value["utilization_cpu"] = json!(round3(self.cpu_utilization()));
        }
        value
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Record started items and schedule their completion.
fn launch(
    tick: u64,
    grants: Vec<Grant<Job>>,
    running: &mut BTreeMap<u64, Vec<Permit>>,
    report: &mut Report,
) {
    for grant in grants {
        report.started += 1;
        if grant.item.large {
            report.large_waits.push(tick - grant.item.submitted);
        }
        running
            .entry(tick + grant.item.ticks)
            .or_default()
            .push(grant.permit);
    }
}

fn simulate(workload: Workload, max_head_skips: u32) -> Report {
    let dispatcher = Dispatcher::in_memory("bench", workload.semaphore(), usize::MAX)
        .with_max_head_skips(max_head_skips);
    let mut running: BTreeMap<u64, Vec<Permit>> = BTreeMap::new();
    let mut report = Report::default();
    let mut rng = Lcg(SEED);
    let mut window = 0;
    for tick in 0..TICKS {
        if let Some(permits) = running.remove(&tick) {
            for permit in permits {
                let started = dispatcher.complete(permit, 0).started;
                launch(tick, started, &mut running, &mut report);
            }
        }
        for (large, demand, ticks) in workload.arrivals(tick, &mut rng) {
            let job = Job {
                large,
                demand,
                ticks,
                submitted: tick,
                sequence: 0,
            };
            let admission = dispatcher.submit(job, 0).expect("unbounded depth");
            launch(
                tick,
                admission.dispatched.started,
                &mut running,
                &mut report,
            );
        }
        let busy = u64::from(dispatcher.units_in_use());
        report.busy_unit_ticks += busy;
        let of = |kind| u64::from(dispatcher.units_in_use_of(kind).unwrap_or(0));
        report.busy_gpu_ticks += of(ResourceKind::GpuVram);
        report.busy_cpu_ticks += of(ResourceKind::Cpu);
        window += busy;
        if (tick + 1).is_multiple_of(WINDOW) {
            report.windows.push(window);
            window = 0;
        }
    }
    report
}

const CONFIGS: [(&str, u32); 6] = [
    ("strict", 0),
    ("skips-2", 2),
    ("skips-4", 4),
    ("skips-8-default", DEFAULT_MAX_HEAD_SKIPS),
    ("skips-16", 16),
    ("unbounded", u32::MAX),
];

fn utilization(c: &mut Criterion) {
    let mut cases = Vec::new();
    for workload in Workload::ALL {
        for (name, skips) in CONFIGS {
            let report = simulate(workload, skips);
            let per_kind = if matches!(workload, Workload::Multi) {
                format!(
                    " (gpu {:.3}, cpu {:.3})",
                    report.gpu_utilization(),
                    report.cpu_utilization()
                )
            } else {
                String::new()
            };
            println!(
                "utilization {:>8} {name:>16}: utilization {:.3}{per_kind}, started {:>5}, \
                 large started {:>4}, large wait p50 {:>4} ticks, max {:>4} ticks",
                workload.name(),
                report.utilization(),
                report.started,
                report.large_waits.len(),
                report.large_wait(0.5),
                report.large_wait(1.0),
            );
            let mut value = report.json(workload);
            value["workload"] = json!(workload.name());
            value["skip_ahead"] = json!(name);
            value["max_head_skips"] = json!(skips);
            cases.push(value);
        }
    }
    if !support::smoke_run() {
        support::write_report(
            "utilization",
            json!({ "ticks": TICKS, "window_ticks": WINDOW, "seed": SEED, "cases": cases }),
        );
    }

    let mut group = c.benchmark_group("utilization_simulation");
    for workload in [Workload::Scalar("mixed", 3), Workload::Multi] {
        for (name, skips) in [CONFIGS[0], CONFIGS[3]] {
            group.bench_with_input(
                BenchmarkId::new(workload.name(), name),
                &skips,
                |b, &skips| b.iter(|| black_box(simulate(workload, skips))),
            );
        }
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = support::criterion_config();
    targets = utilization
);
criterion_main!(benches);
