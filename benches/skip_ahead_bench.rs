//! Skip-ahead utilization benchmark (cr-20).
//!
//! A deterministic, virtual-time simulation of a mixed workload drives the
//! real core `Dispatcher` with skip-ahead off (`max_head_skips` 0, strict
//! head-of-line order), with several guard values, and unbounded. For each it
//! prints, once, the *utilization* (busy unit-ticks over capacity times
//! ticks), the items started, and the wait of the large items (ticks from
//! submission to start, p50 and max); criterion then times one simulation run
//! (the dispatcher's cost, not the workload's).
//!
//! Two workloads, on 16 units for 2,000 ticks, both with a 12-unit, 8-tick
//! item every 9 ticks and, every tick, a fixed-seed random number of small
//! items of 1 or 2 units running 1 to 3 ticks:
//!
//! - `mixed`: 0 to 3 small items per tick, about 95% of capacity offered. A
//!   large item regularly waits at the head for 12 free units.
//! - `overload`: 0 to 5 small items per tick, about 115% offered. Small work
//!   alone can keep more than 4 units busy, so without the guard a large head
//!   can wait indefinitely.
//!
//! ```text
//! cargo bench --bench skip_ahead_bench
//! ```

use std::collections::BTreeMap;
use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};

use prometheus_parking_lot_core::Priority;
use prometheus_parking_lot_core::dispatch::{
    DEFAULT_MAX_HEAD_SKIPS, Dispatcher, Grant, Parkable, Ticket,
};
use prometheus_parking_lot_core::semaphore::{Permit, UnitSemaphore, UnitVector};

const MAX_UNITS: u32 = 16;
const TICKS: u64 = 2_000;

#[derive(Debug)]
struct Job {
    large: bool,
    units: u32,
    ticks: u64,
    submitted: u64,
    sequence: u64,
}

impl Parkable for Job {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: Priority::Normal,
            sequence: self.sequence,
            demand: UnitVector::scalar(self.units),
            deadline_ms: None,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.sequence = sequence;
    }
}

/// Outcome of one simulated run.
#[derive(Debug, Default)]
struct Report {
    busy_unit_ticks: u64,
    started: u64,
    large_waits: Vec<u64>,
}

impl Report {
    fn utilization(&self) -> f64 {
        #[expect(clippy::cast_precision_loss, reason = "values far below 2^52")]
        let ratio = self.busy_unit_ticks as f64 / (TICKS as f64 * f64::from(MAX_UNITS));
        ratio
    }

    fn large_wait(&self, quantile: f64) -> u64 {
        let mut waits = self.large_waits.clone();
        waits.sort_unstable();
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss,
            reason = "index into a short vector"
        )]
        let index = ((waits.len().saturating_sub(1)) as f64 * quantile).round() as usize;
        waits.get(index).copied().unwrap_or(0)
    }
}

/// Fixed-seed linear congruential generator: the same workload every run.
struct Lcg(u64);

impl Lcg {
    const fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
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

fn simulate(max_head_skips: u32, small_per_tick: u64) -> Report {
    let dispatcher = Dispatcher::in_memory("bench", UnitSemaphore::new(MAX_UNITS), usize::MAX)
        .with_max_head_skips(max_head_skips);
    let mut running: BTreeMap<u64, Vec<Permit>> = BTreeMap::new();
    let mut report = Report::default();
    let mut rng = Lcg(0x5eed);
    for tick in 0..TICKS {
        if let Some(permits) = running.remove(&tick) {
            for permit in permits {
                let started = dispatcher.complete(permit, 0).started;
                launch(tick, started, &mut running, &mut report);
            }
        }
        let mut arrivals = Vec::new();
        if tick.is_multiple_of(9) {
            arrivals.push((true, 12, 8));
        }
        for _ in 0..rng.next(small_per_tick + 1) {
            let units = u32::try_from(1 + rng.next(2)).unwrap_or(1);
            arrivals.push((false, units, 1 + rng.next(3)));
        }
        for (large, units, ticks) in arrivals {
            let job = Job {
                large,
                units,
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
        report.busy_unit_ticks += u64::from(dispatcher.units_in_use());
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

/// Workloads: name and the most small items arriving per tick.
const WORKLOADS: [(&str, u64); 2] = [("mixed", 3), ("overload", 5)];

fn skip_ahead_utilization(c: &mut Criterion) {
    for (workload, small_per_tick) in WORKLOADS {
        for (name, skips) in CONFIGS {
            let report = simulate(skips, small_per_tick);
            println!(
                "skip_ahead {workload:>8} {name:>16}: utilization {:.3}, started {:>5}, \
                 large started {:>4}, large wait p50 {:>4} ticks, max {:>4} ticks",
                report.utilization(),
                report.started,
                report.large_waits.len(),
                report.large_wait(0.5),
                report.large_wait(1.0),
            );
        }
    }
    let mut group = c.benchmark_group("skip_ahead_simulation");
    for (name, skips) in CONFIGS {
        group.bench_with_input(BenchmarkId::from_parameter(name), &skips, |b, &skips| {
            b.iter(|| black_box(simulate(skips, 3)));
        });
    }
    group.finish();
}

criterion_group!(benches, skip_ahead_utilization);
criterion_main!(benches);
