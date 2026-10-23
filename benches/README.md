# Benchmarks

Four [criterion](https://docs.rs/criterion) suites (cr-40). The numbers quoted in
`docs/ARCHITECTURE.md` section 7 come from them, with the machine, toolchain and command
next to the numbers.

| Suite | Measures | Source |
|---|---|---|
| `components` | Cost of each piece a submission goes through, one thread: semaphore claim/release (one dimension, and with a per-kind limit), dispatcher submit/complete and park/wake, `InMemoryQueue`, `InMemoryMailbox`, lifecycle-hook emission and its overhead on a pool cycle | `components.rs` |
| `throughput` | Tasks per second through each pool: `ResourcePool`, native `WorkerPool`, `DurableResourcePool` on the in-memory backends and on redb | `throughput.rs` |
| `latency` | Submit-to-start and submit-to-result p50/p95/p99/max per pool under light and saturated load, plus pool utilization under saturation; criterion times one idle round trip | `latency.rs` |
| `utilization` | Units busy over time in a virtual-time simulation of the real dispatcher: strict order against skip-ahead (cr-20), one dimension against cost vectors with per-kind limits (cr-23) | `utilization.rs` |

Each file's module documentation gives the exact workloads. Shared code: `support/mod.rs`
(seeded generator, timeline probe, percentiles, criterion settings, metadata) and
`support/pools.rs` (the pools under test behind one interface).

## Running

The redb cases need the `embedded` feature; run with `--all-features` (what CI does):

```bash
# Full local configuration: 2 s warm-up, 5 s measurement, 50 samples per benchmark.
cargo bench --all-features --locked \
  --bench components --bench throughput --bench latency --bench utilization -- --noplot

# One suite, or the benchmarks matching a name filter.
cargo bench --all-features --locked --bench throughput -- throughput/worker_pool

# Compile and run every benchmark once, no measurement (the report passes run small
# and write nothing).
cargo bench --all-features --locked --bench latency -- --test
```

CI (job `Benchmarks` in `.github/workflows/ci.yml`) runs the same four suites with
`--noplot --warm-up-time 1 --measurement-time 3 --sample-size 20`, then
`.github/scripts/bench-report.py`, which turns the results into the job summary. The
latency percentile pass and the utilization simulation always run in full.

## Where the results go

| What | Where | Override |
|---|---|---|
| criterion estimates, samples, baselines | `target/criterion/` | `CRITERION_HOME` |
| latency percentiles and utilization reports, each with machine metadata | `target/bench-report/{latency,utilization}.json` | `PARKING_LOT_BENCH_REPORT_DIR` |
| Markdown summary of both | stdout of `.github/scripts/bench-report.py --criterion DIR --reports DIR` | `--summary FILE` |

In CI both directories live under `bench-results/` and are uploaded as the artifact
`bench-results-<commit sha>` (kept 90 days).

## Reproducibility

- **Fixed inputs.** Every random workload uses one linear congruential generator with
  seed `0x5eed`; the same build produces the same sequence of costs and arrivals.
- **Virtual time where it matters.** The `utilization` suite advances a tick counter
  instead of reading a clock, so its utilization and wait numbers are identical on every
  machine (only the criterion timing of a simulation run varies).
- **Fixed parallelism.** Async suites run on a tokio runtime with exactly 4 worker
  threads (the vCPU count of a GitHub-hosted Linux runner); pools have 16 units; the
  native `WorkerPool` has 16 threads.
- **Warm-up and measurement.** Locally 2 s warm-up, 5 s measurement, 50 samples,
  noise threshold 5% (`support::criterion_config`); CI shortens these on the command
  line. The durable cases use criterion's flat sampling (their iterations take
  milliseconds to a second).
- **Metadata.** `latency.json` and `utilization.json` record OS, architecture, CPU
  model, logical CPUs, memory, the load average when the report was written,
  `rustc --version`, the profile, the runtime worker count, the commit and the CI
  runner name. A number is only comparable with another taken on the same machine at
  a similar load.
- **Variance.** The latency pass runs 5 rounds per pool and workload, each on a fresh
  pool, and reports per statistic the median and the minimum and maximum across
  rounds. Criterion reports a 95% confidence interval per benchmark.

What these suites do not control: other processes on the machine (the numbers in
`docs/ARCHITECTURE.md` were taken with other builds running), CPU frequency scaling, and
the storage device behind `$TMPDIR` (the redb cases commit with fsync to a store in a
temporary directory, so they measure the disk as much as the scheduler).

## Regression policy (report-only)

On `main`, CI saves the run as criterion's `main` baseline (`--save-baseline main`,
stored with `actions/cache` under `bench-baseline-<sha>`). A pull request restores the
newest `main` baseline and runs with `--baseline-lenient main`, so each benchmark's
change appears in the job summary with its confidence interval. A benchmark whose
whole 95% interval of change lies above **+25%** gets a `::warning::` annotation. The
job never fails on a number.

Why report-only, and why 25%: two back-to-back runs of identical code at the CI
configuration on one machine (M1 Max, load average 20 to 30, 2026-10-23) produced
changes whose 95% intervals excluded zero by up to 22%: for example
`throughput/resource_pool/mixed` -21.7% [-31.2%, -12.5%] and
`hook_pool_cycle/noop_hook` +8.3% [+4.4%, +12.7%]. Between CI runs the variance is
larger, not smaller: a pull request and its `main` baseline usually run on different
runner hosts, possibly with different CPU models. A gate tighter than that noise
would fail unchanged code; a much looser one would never fire. A slower number is a
prompt to re-run both sides on one machine, not a verdict.

## Postgres

No Postgres case is included: the lease queue and mailbox need Docker
(`crates/parking-lot-pg/tests`), and a benchmark that silently skips is worse than
none. Adding one is follow-up work: a Docker-gated `throughput/durable_pg` case behind
the `postgres` feature, run by the `postgres` CI job.
