//! Shared pieces of the benchmark suites (cr-40): the fixed-seed generator,
//! the per-task timeline probe, percentiles, the criterion configuration and
//! the machine/toolchain metadata written next to every report.
//!
//! Every suite writes a JSON report to `$PARKING_LOT_BENCH_REPORT_DIR`
//! (default `target/bench-report`, relative to the package root, which is the
//! working directory `cargo bench` gives a bench binary). Criterion's own
//! results go to `$CRITERION_HOME` (default `target/criterion`).

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use criterion::Criterion;
use serde_json::{Value, json};
use tokio::sync::Notify;

/// Criterion settings of the full local run. CI passes shorter
/// `--warm-up-time`, `--measurement-time` and `--sample-size` on the command
/// line, which criterion applies on top of these.
pub fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(2))
        .measurement_time(Duration::from_secs(5))
        .sample_size(50)
        .noise_threshold(0.05)
}

/// A tokio runtime with a fixed number of workers, so the suites do not scale
/// with the machine's core count (CI runners have 4 vCPUs).
pub const RUNTIME_WORKERS: usize = 4;

/// The runtime every async suite runs on.
pub fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(RUNTIME_WORKERS)
        .enable_all()
        .build()
        .expect("build the benchmark runtime")
}

/// Fixed-seed linear congruential generator: the same workload every run.
pub struct Lcg(pub u64);

impl Lcg {
    /// The next value in `0..bound`.
    pub const fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % bound
    }
}

/// Per-task timestamps (nanoseconds since the probe was built; 0 = not yet)
/// and a completion counter a client can wait on.
///
/// Stamps are `Release` stores and are read only after [`Probe::wait_for`]
/// returned, whose `SeqCst` load of the counter synchronizes with the
/// `SeqCst` increment in [`Probe::finished`]. Each task's `submitted`,
/// `started` and `ended` stamps are sequenced before (or ordered by the pool's
/// own channel or lock hand-off before) its `finished` increment, so every
/// stamp of a counted task is visible to the reader.
pub struct Probe {
    base: Instant,
    submitted: Box<[AtomicU64]>,
    started: Box<[AtomicU64]>,
    ended: Box<[AtomicU64]>,
    finished_at: Box<[AtomicU64]>,
    count: AtomicU64,
    target: AtomicU64,
    done: Notify,
}

fn slots(n: usize) -> Box<[AtomicU64]> {
    (0..n).map(|_| AtomicU64::new(0)).collect()
}

impl Probe {
    /// A probe with timestamp slots for task ids `0..tasks` (0 slots for a
    /// probe that only counts completions).
    pub fn new(tasks: usize) -> Self {
        Self {
            base: Instant::now(),
            submitted: slots(tasks),
            started: slots(tasks),
            ended: slots(tasks),
            finished_at: slots(tasks),
            count: AtomicU64::new(0),
            target: AtomicU64::new(u64::MAX),
            done: Notify::new(),
        }
    }

    fn stamp(&self) -> u64 {
        u64::try_from(self.base.elapsed().as_nanos())
            .unwrap_or(u64::MAX)
            .max(1)
    }

    fn mark(&self, slot: &[AtomicU64], id: u64) {
        if let Some(cell) = usize::try_from(id).ok().and_then(|i| slot.get(i)) {
            cell.store(self.stamp(), Ordering::Release);
        }
    }

    /// The client is about to call `submit` for `id`.
    pub fn submitted(&self, id: u64) {
        self.mark(&self.submitted, id);
    }

    /// The executor began running `id`.
    pub fn started(&self, id: u64) {
        self.mark(&self.started, id);
    }

    /// The executor returned for `id`.
    pub fn ended(&self, id: u64) {
        self.mark(&self.ended, id);
    }

    /// The result of `id` is where its client reads it.
    pub fn finished(&self, id: u64) {
        self.mark(&self.finished_at, id);
        // SeqCst on this increment and on the target load below, paired with
        // the SeqCst target store and count load in `wait_for`: in the single
        // total order either the waiter's count load sees this increment or
        // this target load sees the waiter's target (store-buffering pattern),
        // so the last completion is never missed.
        let count = self.count.fetch_add(1, Ordering::SeqCst) + 1;
        if count >= self.target.load(Ordering::SeqCst) {
            // `notify_one` stores a permit when nobody waits yet: no lost wakeup.
            self.done.notify_one();
        }
    }

    /// Completions so far.
    pub fn count(&self) -> u64 {
        self.count.load(Ordering::SeqCst)
    }

    /// Wait until `target` completions have been counted.
    pub async fn wait_for(&self, target: u64) {
        self.target.store(target, Ordering::SeqCst);
        // A permit left by an earlier round only causes one extra check.
        while self.count.load(Ordering::SeqCst) < target {
            self.done.notified().await;
        }
    }

    fn read(slot: &[AtomicU64], id: usize) -> u64 {
        slot[id].load(Ordering::Acquire)
    }

    /// Per-task latencies of tasks `0..tasks`, in microseconds.
    pub fn latencies(&self, tasks: usize) -> Latencies {
        let mut to_start = Vec::with_capacity(tasks);
        let mut to_result = Vec::with_capacity(tasks);
        for id in 0..tasks {
            let submitted = Self::read(&self.submitted, id);
            let started = Self::read(&self.started, id);
            let finished = Self::read(&self.finished_at, id);
            assert!(
                submitted > 0 && started > 0 && finished > 0,
                "task {id} has a missing stamp"
            );
            to_start.push(micros(started.saturating_sub(submitted)));
            to_result.push(micros(finished.saturating_sub(submitted)));
        }
        Latencies {
            to_start,
            to_result,
        }
    }

    /// Unit-weighted busy fraction of `units` over the span from the first
    /// submission to the last result: sum of (executor end - executor start)
    /// times `cost`, over `units` times the span.
    pub fn utilization(&self, tasks: usize, cost: u32, units: u32) -> f64 {
        let first = (0..tasks)
            .map(|id| Self::read(&self.submitted, id))
            .min()
            .unwrap_or(0);
        let last = (0..tasks)
            .map(|id| Self::read(&self.finished_at, id))
            .max()
            .unwrap_or(0);
        let busy: u64 = (0..tasks)
            .map(|id| {
                Self::read(&self.ended, id).saturating_sub(Self::read(&self.started, id))
                    * u64::from(cost)
            })
            .sum();
        ratio(busy, last.saturating_sub(first) * u64::from(units))
    }
}

/// `numerator / denominator` as a float (0 when the denominator is 0).
pub fn ratio(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        return 0.0;
    }
    #[expect(clippy::cast_precision_loss, reason = "report values far below 2^52")]
    let value = numerator as f64 / denominator as f64;
    value
}

fn micros(nanos: u64) -> f64 {
    #[expect(clippy::cast_precision_loss, reason = "latencies far below 2^52 ns")]
    let value = nanos as f64 / 1_000.0;
    value
}

/// Per-task latencies of one round, in microseconds.
pub struct Latencies {
    /// Submission to the executor starting.
    pub to_start: Vec<f64>,
    /// Submission to the result being readable by the client.
    pub to_result: Vec<f64>,
}

/// The `q` quantile (nearest rank) of `values`.
pub fn quantile(values: &[f64], q: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    if sorted.is_empty() {
        return 0.0;
    }
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "index into a short vector"
    )]
    let rank = ((sorted.len() as f64 * q).ceil() as usize).clamp(1, sorted.len());
    sorted[rank - 1]
}

/// p50, p95, p99 and max of `values`.
pub fn percentiles(values: &[f64]) -> Value {
    json!({
        "p50": round2(quantile(values, 0.50)),
        "p95": round2(quantile(values, 0.95)),
        "p99": round2(quantile(values, 0.99)),
        "max": round2(quantile(values, 1.0)),
    })
}

/// Round to two decimals for the report.
pub fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

/// Median, minimum and maximum of one statistic across rounds.
pub fn spread(values: &[f64]) -> Value {
    json!({
        "median": round2(quantile(values, 0.5)),
        "min": round2(quantile(values, 0.0)),
        "max": round2(quantile(values, 1.0)),
    })
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn first_line_with(path: &str, prefix: &str) -> Option<String> {
    fs::read_to_string(path).ok()?.lines().find_map(|line| {
        line.starts_with(prefix).then(|| {
            line.split_once(':')
                .map_or("", |(_, v)| v)
                .trim()
                .to_owned()
        })
    })
}

/// Machine, toolchain and run metadata for a report. The load average is
/// read when the report is written: these suites are sensitive to other work
/// on the machine, so a number is only comparable with its load next to it.
pub fn metadata() -> Value {
    let (cpu, memory, load) = if cfg!(target_os = "macos") {
        (
            command_output("sysctl", &["-n", "machdep.cpu.brand_string"]),
            command_output("sysctl", &["-n", "hw.memsize"]),
            command_output("sysctl", &["-n", "vm.loadavg"]),
        )
    } else {
        (
            first_line_with("/proc/cpuinfo", "model name"),
            first_line_with("/proc/meminfo", "MemTotal"),
            fs::read_to_string("/proc/loadavg")
                .ok()
                .map(|s| s.trim().to_owned()),
        )
    };
    json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "cpu": cpu,
        "logical_cpus": std::thread::available_parallelism().map_or(0, std::num::NonZero::get),
        "memory": memory,
        "load_average": load,
        "rustc": command_output("rustc", &["--version"]),
        "profile": if cfg!(debug_assertions) { "debug" } else { "bench (optimized)" },
        "runtime_workers": RUNTIME_WORKERS,
        "commit": std::env::var("GITHUB_SHA").ok().or_else(|| command_output("git", &["rev-parse", "HEAD"])),
        "runner": std::env::var("RUNNER_NAME").ok(),
        "unix_time_s": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    })
}

/// True when criterion was asked to list or test the benchmarks rather than
/// measure them (`--list`, `--test`, or `cargo test --benches`): the report
/// passes then run small and write nothing.
pub fn smoke_run() -> bool {
    std::env::args().any(|arg| arg == "--list" || arg == "--test")
        || !std::env::args().any(|arg| arg == "--bench")
}

/// Write `report` (with metadata added) as `<dir>/<suite>.json` and print
/// where it went.
pub fn write_report(suite: &str, mut report: Value) {
    report["suite"] = json!(suite);
    report["metadata"] = metadata();
    let dir = std::env::var_os("PARKING_LOT_BENCH_REPORT_DIR")
        .map_or_else(|| PathBuf::from("target/bench-report"), PathBuf::from);
    let path = dir.join(format!("{suite}.json"));
    let text = serde_json::to_string_pretty(&report).expect("serialize the report");
    match fs::create_dir_all(&dir).and_then(|()| fs::write(&path, text)) {
        Ok(()) => println!("{suite}: report written to {}", path.display()),
        Err(error) => eprintln!("{suite}: cannot write {}: {error}", path.display()),
    }
}
