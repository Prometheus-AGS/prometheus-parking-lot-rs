---
type: SessionRecord
id: karpathy-session-043a846768cf1d82
title: Karpathy session 043a846768cf
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T20:52:07.720719+00:00
created_at: 2026-10-23T20:52:07.720719+00:00
updated_at: 2026-10-23T20:52:07.720719+00:00
revision: 0
---

## Delta

I've filed 20 issues on the 0.4.0 milestone, #52–#71. Each has the `team:parking-lot-core` label plus `enhancement`, `testing` or `documentation`, says which PR found the gap, and says what "done" means. One issue timed out the first time, so I checked it hadn't been created before retrying. The milestone has exactly 20 open issues.

**Enhancements** (12):
- #52 Config-built durable pools can't take a notifier
- #53 Config-built durable pools: result TTL, heartbeat and poll intervals aren't configurable
- #54 Durable pools wake on Postgres LISTEN/NOTIFY instead of polling
- #55 Allow a config with no pools (events only)
- #56 Optional `result_ttl_ms` for `WorkerPool`
- #57 Lifecycle event for tasks removed because they expired
- #58 Skip-ahead for durable pools
- #59 Durable mode for the native `WorkerPool`
- #60 Decide on and test durable pools on pgmq
- #61 Attach the Iggy hook from config
- #62 A restarted Iggy publisher can store duplicate messages
- #63 Recover the 2–4% single-resource slowdown from cr-23

**Testing** (7):
- #64 Formal concurrency models (loom) for the shutdown ledger and the per-resource release path
- #65 Run the WASM tests in a headless browser
- #66 Test the Postgres 17→18 upgrade in the runbook
- #67 Run the Tauri app's GUI and build it on Linux and Windows
- #68 Docker test for a stream that stops after a failed send
- #69 Remove a stale test and its `reqwest` dependency
- #71 Benchmark Postgres, contention and memory

**Documentation** (1):
- #70 Mark ADRs 0001 and 0003 as superseded and fix a stale CI step name

When this phase closes, its reflection will turn these issues into the goals for the next phase.

**Benchmarks are up as PR #51 (cr-40):** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/51. CI is still running, so please hold the merge. This is the last planned change of the phase.
- I merged the guides (#50) into it first. The only conflict was the CHANGELOG, and I kept both entries. Every check passes on the merged code.
- Main numbers from this Mac, which was heavily loaded, so treat them as upper bounds:
  - **Throughput:** about 470K tasks/s for the in-memory `ResourcePool` and about 43/s for the redb-backed durable pool. Each durable task needs at least four writes flushed to disk.
  - **Latency:** about 6 µs from submit to result under light load.
  - **Skip-ahead:** raises capacity use from 0.917 to 0.947.
- The CI benchmark job only reports. It never fails a build, because reruns of identical code on this Mac moved by up to 70%.

**Still running:** tech-lead's batch of three pre-release API fixes.

**Local cleanup you can do:** the benchmark agent left run output in the ignored `target/` folder. To clear it:
`! rm -rf /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/scheduler-engineer/target/cr40 /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/scheduler-engineer/target/cr40-*.log`

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T20:51:17.737872Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
