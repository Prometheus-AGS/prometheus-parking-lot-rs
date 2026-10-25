# Benchmarks

Four [criterion](https://docs.rs/criterion) suites (cr-40), plus the separate
fixed-workload Postgres executable `pg_backend` (OA-17). The historical numbers
quoted in `docs/ARCHITECTURE.md` section 7 come from cr-40, with the machine,
toolchain and command next to the numbers. OA-17 has no measured results yet.

| Suite | Measures | Source |
|---|---|---|
| `components` | Cost of each piece a submission goes through, one thread: semaphore claim/release (one dimension, and with a per-kind limit), dispatcher submit/complete and park/wake, `InMemoryQueue`, `InMemoryMailbox`, lifecycle-hook emission and its overhead on a pool cycle | `components.rs` |
| `throughput` | Tasks per second through each pool: `ResourcePool`, native `WorkerPool`, `DurableResourcePool` on the in-memory backends and on redb | `throughput.rs` |
| `latency` | Submit-to-start and submit-to-result p50/p95/p99/max per pool under light and saturated load, plus pool utilization under saturation; criterion times one idle round trip | `latency.rs` |
| `utilization` | Units busy over time in a virtual-time simulation of the real dispatcher: strict order against skip-ahead (cr-20), one dimension against cost vectors with per-kind limits (cr-23) | `utilization.rs` |
| `pg_backend` | Real PG queue/mailbox/durable client cycles with 1/8 clients, paired baseline/candidate raw samples and client-process peak RSS; separate from Criterion | `pg_backend.rs`, `pg_measure.py` |

Each file's module documentation gives the exact workloads. Shared Criterion code: `support/mod.rs`
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

## Historical Criterion regression policy (report-only)

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
`hook_pool_cycle/noop_hook` +8.3% [+4.4%, +12.7%]. A third run, after merging a
change that does not touch these paths, reported `latency_round_trip/resource_pool`
+27.1% [+15.1%, +38.8%] and `hook_pool_cycle/no_hooks` +69.9% [+12.6%, +134.9%]: a
15% threshold would have warned on both, 25% on neither. Between CI runs the variance is
larger, not smaller: a pull request and its `main` baseline usually run on different
runner hosts, possibly with different CPU models. A gate tighter than that noise
would fail unchanged code; a much looser one would never fire. A slower number is a
prompt to re-run both sides on one machine, not a verdict.

## Postgres

OA-17 adds a separate `harness = false` target, `pg_backend`, requiring
`tokio-runtime,postgres`. It is not silently skipped or added to the historical
Criterion CI matrix. The implementation and commands below are **NOT_EXERCISED**;
OA-18 owns execution after the full production set. No PG throughput, latency,
memory improvement or regression is claimed before that execution.

| Fixed case | One measured operation |
|---|---|
| `queue` | PgLeaseQueue enqueue, lease and ack; concurrent clients may lease one another's entries, and the complete task-id set must be observed once. This is a client cycle, not per-task queue residence time. |
| `mailbox_unique` | A fresh idempotent Completed delivery, peek, then consume; each client has its own address. |
| `mailbox_duplicate` | Duplicate delivery against one preseeded shared address/caller-key fence; all clients contend on the same key and the original result must remain unchanged. |
| `durable` | DurableResourcePool submit through actual PG queue/executor/mailbox to client consumption; lease/accounting drain is verified after the timed phase. |

Each process uses four Tokio worker threads, 1 or 8 concurrent clients,
1,024 measured operations after 128 warm-up operations, 256-byte payloads,
Normal priority, one-unit CPU costs, a 16-unit durable pool, a 30s lease and
1ms polling. The measured SQLx pool maximum must be 10; drift is a workload
identity failure. IDs are fixed sequential inputs, not random arrivals.
Latencies include client work, assertions, storage calls and polling. Setup,
migrations, warm-up, final consistency checks and shutdown are outside the
Rust timed phase. The process peak RSS includes all those phases.

The identical harness is copied into a clean disposable baseline checkout at
`92da0d3d28f7896ddc67062fa49ba40ea48d74a3` (0.3.0), with only the approved
bench-target manifest injection. Production code and Cargo.lock stay original.
Candidate production code must be frozen at its full commit SHA. The harness
compares only `Stored`, so baseline's unit Duplicate and candidate's
Duplicate-with-status compile from the same source without a semantic shim.
Harness/tool digests, features, profile, rustc and Cargo must match across arms;
the production source, manifest, lock and executable identities are retained
separately. Each sample uses a fresh disposable PG container/database, so each
binary applies its own schema rather than sharing or relabeling a store.

### Quiet-machine protocol and exact commands

Use a provisioned Linux or macOS host with Python3, Unix wait4, the pinned Rust
toolchain/dependencies, Docker and the already-built project PG image. There is
no image pull/build or global installation fallback. Keep one Cargo writer.
Finish all source changes, image preparation and both builds before the quiet
measurement interval. No agents, builds or other benchmarks may run throughout
collection; do not tune the host or clear caches. Pair order alternates to
reduce order bias, without pretending to eliminate filesystem/cache/frequency
effects. The default envelope is two hours per capture, with a 2,400s child
watchdog and 120s Docker-command watchdogs; timeout is unavailable/failing
evidence, never a performance sample.

Set the following inputs to real paths/identities before running: a clean
baseline scratch root (`OA17_BASELINE_ROOT`), the clean frozen candidate root
(`OA17_CANDIDATE_ROOT`), its full SHA (`OA17_CANDIDATE_SHA`), a fresh evidence
directory outside both source trees (`OA17_EVIDENCE_ROOT`), and an operator's
quiet-host attestation copy (`OA17_QUIET_ATTESTATION`). Create the evidence
parent directory first. The example JSON starts with false/empty fields and
cannot authorize collection; fill the copy truthfully only after stopping
agents/builds. Process-name snapshots reject known agent/build processes at
sample boundaries, but cannot prove the absence of every background job or
activity between snapshots. The operator attests the entire interval.

From the frozen candidate root, stage and build before that quiet interval:

```bash
python3 benches/pg_measure.py stage-baseline --root "$OA17_BASELINE_ROOT" --candidate-root "$OA17_CANDIDATE_ROOT" --out "$OA17_EVIDENCE_ROOT/staging.json"
python3 benches/pg_measure.py build --arm baseline --root "$OA17_BASELINE_ROOT" --revision 92da0d3d28f7896ddc67062fa49ba40ea48d74a3 --out "$OA17_EVIDENCE_ROOT/baseline-build.json"
python3 benches/pg_measure.py build --arm candidate --root "$OA17_CANDIDATE_ROOT" --revision "$OA17_CANDIDATE_SHA" --out "$OA17_EVIDENCE_ROOT/candidate-build.json"
```

Build refuses ambient `RUSTFLAGS`/`CARGO_ENCODED_RUSTFLAGS`, records the actual
linked artifact and compiler profile, and uses a separate
`target/oa17-build` in each source root. Its exact Cargo argv is:

```bash
cargo bench -p prometheus_parking_lot --locked --no-default-features --features tokio-runtime,postgres --bench pg_backend --no-run --message-format=json
```

After stopping agents/builds and preparing the truthful attestation, collect
normal and control samples into different new directories:

```bash
python3 benches/pg_measure.py measure --baseline-receipt "$OA17_EVIDENCE_ROOT/baseline-build.json" --candidate-receipt "$OA17_EVIDENCE_ROOT/candidate-build.json" --image parking-lot-pg:18 --quiet-attestation "$OA17_QUIET_ATTESTATION" --samples 5 --max-wall-seconds 7200 --out "$OA17_EVIDENCE_ROOT/normal"
python3 benches/pg_measure.py report --capture "$OA17_EVIDENCE_ROOT/normal" --out "$OA17_EVIDENCE_ROOT/report.json"
python3 benches/pg_measure.py measure --baseline-receipt "$OA17_EVIDENCE_ROOT/baseline-build.json" --candidate-receipt "$OA17_EVIDENCE_ROOT/candidate-build.json" --image parking-lot-pg:18 --quiet-attestation "$OA17_QUIET_ATTESTATION" --samples 5 --max-wall-seconds 7200 --slow-control --out "$OA17_EVIDENCE_ROOT/slow-control"
python3 benches/pg_measure.py report --capture "$OA17_EVIDENCE_ROOT/slow-control" --allow-slow-control --out "$OA17_EVIDENCE_ROOT/slow-control-report.json"
python3 benches/pg_measure_test.py -v
```

The normal matrix is 80 processes: four cases × two client counts × five
pairs × two arms. Each cell has five baseline and five candidate samples.
The separate slow matrix is ten processes: hot-key mailbox, eight clients,
five pairs, with an explicitly recorded 100ms candidate delay per measured
operation. It changes the control's client behavior deliberately; a normal
report rejects it. Actual slowed real-PG collection and classification are
required. The eight synthetic `ReportControls` tests support validator/math
checks only and cannot replace either real capture. To measure another PG
major, repeat into new directories using its prebuilt immutable image; do not
combine different images or machines into one comparison.

### Evidence, interpretation and limits

Retain the complete capture directory and build/staging receipts. The capture
contains per-operation latency arrays, verified operation counts and elapsed
time; redacted raw client stdout/stderr; raw build logs; source, harness,
tool, executable and log hashes; full baseline/candidate revisions; actual
compiler/features/profile; OS/CPU/thread/memory/Python identity; Docker version,
VM resources, immutable image ID/repository digests, actual PG version and
container limits; process snapshots, load observations and timestamps.
Dropping a raw sample/file, changing workload or machine/image identity,
losing raw latencies or changing bound logs rejects report validation. Failed
processes, zero discovery, timeouts and unavailable tooling do not become
samples. Freeze the capture manifest digest in the final candidate/contract
evidence packet and retain raw artifacts with it.

Peak RSS is the exact benchmark-client/pool process wait4 maximum, normalized
from Linux KiB or macOS bytes. It includes setup/migration/warm-up/shutdown
and excludes the PostgreSQL server, Docker daemon/VM and Python runner.
Docker VM memory capacity is comparability metadata, not RSS or a cgroup
memory measurement. This is the OA-17 client-memory scope; it makes no
database-process memory claim.

Reports give raw per-process throughput, p50/p95/p99 and RSS samples, paired
ratios and deterministic 95% paired-bootstrap cost-ratio intervals (10,000
resamples, seed `0x5eed`). An interval wholly above 1 marks an observed cost
regression; wholly below 1 marks improvement; overlap is inconclusive.
Throughput ratios are inverted for cost interpretation. There is no new
release threshold or blanket regression budget, and no gain claim where
uncertainty overlaps noise. Five-pair bootstrap evidence and a quiet-host
attestation do not establish universal performance or control every system
effect. The historical Criterion +25% warning policy above does not govern
this separate comparison. Report every observed regression for the
operator's release decision; successful report validation is not release
certification. All OA-17 measurements remain **NOT_EXERCISED** until OA-18.
