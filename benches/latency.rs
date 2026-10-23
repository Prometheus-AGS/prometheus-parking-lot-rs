//! Dispatch latency (cr-40): submit-to-start and submit-to-result
//! percentiles of each pool, under light and saturated load.
//!
//! Two passes:
//!
//! 1. **Percentile report** (printed, and written to
//!    `target/bench-report/latency.json`). Every pool has 16 units; for each
//!    workload it runs `ROUNDS` rounds on a fresh pool and records, per task,
//!    the submission, the executor start and end, and the moment the result is
//!    readable (`support/pools.rs`). Per round: p50, p95, p99 and max of
//!    submit-to-start and submit-to-result, in microseconds, and the
//!    unit-weighted utilization of the 16 units from the first submission to
//!    the last result. The report gives every round and, per statistic, the
//!    median, minimum and maximum across rounds (the spread is the run-to-run
//!    variance).
//!
//!    | workload | arrivals | service time | tasks (in-memory / durable-memory / redb) |
//!    |---|---|---|---|
//!    | `light` | one at a time: the next is submitted when the previous result is readable | none | 2,000 / 400 / 50 |
//!    | `saturated` | the whole round at once, many times the pool's capacity | 1 ms (tokio sleep) holding 1 unit | 1,600 / 320 / 96 |
//!
//!    Under `light` the numbers are the scheduler's overhead on an idle pool.
//!    Under `saturated` submit-to-start is dominated by queueing: a task waits
//!    for every task ahead of it, so the expected p50 is about half the
//!    makespan. The utilization column says how close the pool came to keeping
//!    16 units busy (1.0 = never an idle unit while work was waiting).
//!
//! 2. **Criterion** (`latency_round_trip/<pool>`): the time of one
//!    submit-to-result round trip on an idle pool, with criterion's confidence
//!    interval and baseline comparison.
//!
//! ```text
//! cargo bench --all-features --bench latency
//! ```

#[allow(dead_code, reason = "each suite uses a different part of the shared module")]
mod support;
#[path = "support/pools.rs"]
mod pools;

use std::sync::Arc;
use std::time::{Duration, Instant};

use criterion::{Criterion, SamplingMode, criterion_group, criterion_main};
use serde_json::{Value, json};

use pools::{Harness, Kind};
use support::{Probe, percentiles, spread};

/// Units of every pool.
const UNITS: u32 = 16;
/// Rounds per pool and workload.
const ROUNDS: usize = 5;
/// Service time of a saturated-workload task.
const SERVICE: Duration = Duration::from_millis(1);

/// A load shape.
#[derive(Debug, Clone, Copy)]
enum Workload {
    Light,
    Saturated,
}

impl Workload {
    const ALL: [Self; 2] = [Self::Light, Self::Saturated];

    const fn name(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Saturated => "saturated",
        }
    }

    /// Tasks per round. Below the in-memory mailbox's key space (4,096), so
    /// every task has its own key.
    const fn tasks(self, kind: Kind) -> usize {
        match (self, kind) {
            (Self::Light, Kind::Resource | Kind::Worker) => 2_000,
            (Self::Saturated, Kind::Resource | Kind::Worker) => 1_600,
            (Self::Light, Kind::DurableMemory) => 400,
            (Self::Saturated, Kind::DurableMemory) => 320,
            #[cfg(feature = "embedded")]
            (Self::Light, Kind::DurableRedb) => 50,
            #[cfg(feature = "embedded")]
            (Self::Saturated, Kind::DurableRedb) => 96,
        }
    }

    const fn service(self) -> Duration {
        match self {
            Self::Light => Duration::ZERO,
            Self::Saturated => SERVICE,
        }
    }
}

/// One round on a fresh pool: per-round statistics.
fn round(runtime: &tokio::runtime::Runtime, kind: Kind, workload: Workload, tasks: usize) -> Value {
    let probe = Arc::new(Probe::new(tasks));
    let probe_in = Arc::clone(&probe);
    runtime.block_on(async move {
        let harness = Harness::new(kind, Arc::clone(&probe_in), workload.service(), UNITS);
        let ids = 0..u64::try_from(tasks).expect("small task count");
        match workload {
            Workload::Light => {
                for id in ids {
                    harness.submit(id, 1).await;
                    probe_in.wait_for(id + 1).await;
                }
            }
            Workload::Saturated => {
                for id in ids.clone() {
                    harness.submit(id, 1).await;
                }
                probe_in.wait_for(ids.end).await;
            }
        }
    });
    let latencies = probe.latencies(tasks);
    json!({
        "submit_to_start_us": percentiles(&latencies.to_start),
        "submit_to_result_us": percentiles(&latencies.to_result),
        "utilization": (probe.utilization(tasks, 1, UNITS) * 1000.0).round() / 1000.0,
    })
}

/// Median, min and max across rounds of `rounds[*][metric][stat]`.
fn across(rounds: &[Value], metric: &str, stat: &str) -> Value {
    let values: Vec<f64> = rounds
        .iter()
        .filter_map(|round| {
            if stat.is_empty() {
                round[metric].as_f64()
            } else {
                round[metric][stat].as_f64()
            }
        })
        .collect();
    spread(&values)
}

/// The percentile pass: every pool, every workload, `ROUNDS` rounds.
fn percentile_report(runtime: &tokio::runtime::Runtime) {
    let smoke = support::smoke_run();
    let mut cases = Vec::new();
    for &kind in Kind::ALL {
        for workload in Workload::ALL {
            let (tasks, rounds) = if smoke {
                (20, 1)
            } else {
                (workload.tasks(kind), ROUNDS)
            };
            let started = Instant::now();
            let results: Vec<Value> = (0..rounds)
                .map(|_| round(runtime, kind, workload, tasks))
                .collect();
            let summary = json!({
                "submit_to_start_us": {
                    "p50": across(&results, "submit_to_start_us", "p50"),
                    "p95": across(&results, "submit_to_start_us", "p95"),
                    "p99": across(&results, "submit_to_start_us", "p99"),
                },
                "submit_to_result_us": {
                    "p50": across(&results, "submit_to_result_us", "p50"),
                    "p95": across(&results, "submit_to_result_us", "p95"),
                    "p99": across(&results, "submit_to_result_us", "p99"),
                },
                "utilization": across(&results, "utilization", ""),
            });
            println!(
                "latency {:>14} {:>9}: start p50 {:>9.1} p99 {:>9.1} us | result p50 {:>9.1} p99 {:>9.1} us | util {:.3} ({} tasks x {} rounds, {:.1}s)",
                kind.name(),
                workload.name(),
                summary["submit_to_start_us"]["p50"]["median"].as_f64().unwrap_or(0.0),
                summary["submit_to_start_us"]["p99"]["median"].as_f64().unwrap_or(0.0),
                summary["submit_to_result_us"]["p50"]["median"].as_f64().unwrap_or(0.0),
                summary["submit_to_result_us"]["p99"]["median"].as_f64().unwrap_or(0.0),
                summary["utilization"]["median"].as_f64().unwrap_or(0.0),
                tasks,
                rounds,
                started.elapsed().as_secs_f64(),
            );
            cases.push(json!({
                "pool": kind.name(),
                "workload": workload.name(),
                "units": UNITS,
                "tasks": tasks,
                "service_us": workload.service().as_micros(),
                "rounds": results,
                "summary": summary,
            }));
        }
    }
    if !smoke {
        support::write_report("latency", json!({ "cases": cases }));
    }
}

fn latency(c: &mut Criterion) {
    let runtime = support::runtime();
    percentile_report(&runtime);

    let mut group = c.benchmark_group("latency_round_trip");
    for &kind in Kind::ALL {
        let probe = Arc::new(Probe::new(0));
        let harness = {
            let _guard = runtime.enter();
            Arc::new(Harness::new(kind, Arc::clone(&probe), Duration::ZERO, UNITS))
        };
        let mut next_id = 0_u64;
        group.sampling_mode(if kind.is_durable() {
            SamplingMode::Flat
        } else {
            SamplingMode::Auto
        });
        group.bench_function(kind.name(), |b| {
            b.to_async(&runtime).iter_custom(|iters| {
                let harness = Arc::clone(&harness);
                let probe = Arc::clone(&probe);
                let first = next_id;
                next_id += iters;
                async move {
                    let begin = Instant::now();
                    for id in first..first + iters {
                        let target = probe.count() + 1;
                        harness.submit(id, 1).await;
                        probe.wait_for(target).await;
                    }
                    begin.elapsed()
                }
            });
        });
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = support::criterion_config();
    targets = latency
);
criterion_main!(benches);
