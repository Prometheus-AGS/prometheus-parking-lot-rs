---
type: SessionRecord
id: karpathy-session-2bde2bf01b95bf77
title: Karpathy session 2bde2bf01b95
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T14:46:36.624128+00:00
created_at: 2026-10-23T14:46:36.624128+00:00
updated_at: 2026-10-23T14:46:36.624128+00:00
revision: 0
---

## Delta

Round 10 is merged, and `main` passed every check at `5e601fe`, including the first runs of the new Postgres 17 and 18 jobs.

**PR #38, a small follow-up to merge:** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/38
- #36 and #37 were merged in parallel, so CI never ran the Postgres crash tests from #37. Those tests were hard-coded to Postgres 18 and weren't in the new Postgres job's test list.
- #38 makes them follow the Postgres 17/18 setting and adds them to that job.
- I ran them locally: 4 of 4 pass on Postgres 17 and 4 of 4 on Postgres 18.
- CI on #38 is still running. I'll confirm it before you merge.

**KBD:** the implementation tasks for cr-36 and cr-49 are closed.

**Round 11 has started.** scheduler-engineer is on cr-29, shutdown and cancellation, running alone because this change is high complexity. When a pool shuts down, it stops taking new tasks and lets running ones finish up to a deadline. Every task it already accepted then ends one of three ways: it completes, it is recorded as cancelled, or it goes back on the queue. It also fixes a known bug where a task already taken off the queue is dropped when shutdown is noticed. The test counts every task to make sure none goes missing. The negative control puts that bug back and checks that the test catches it. Tasks put back on the queue must run after a restart, on both Postgres and redb.

**Small follow-ups not yet assigned:**
- A Tokio timer and spawner to pair with the cr-34 background notifier.
- An optional `result_ttl_ms` config setting.
- A pgmq backend type in the config.
- A one-line lint fix in the embedded crate, `fault.rs:45`. It only fails when that crate is linted on its own, which CI doesn't do.

I'll fold these into the dependency-cleanup round, cr-39, unless you'd rather have them sooner.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T14:46:34.920159Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- .kbd-orchestrator/current-waypoint.json
- .kbd-orchestrator/phases/phase-certification-remediation/progress.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/hooks-status.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/hooks.log.jsonl
- .kbd-orchestrator/phases/phase-correctness-and-durability/progress.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/tasks.md
- .kbd-orchestrator/position-reminder.txt
- .kbd-orchestrator/position.json
- openspec/changes/cr-16-docs-truth/tasks.md
- openspec/changes/cr-21-panic-containment/tasks.md
- openspec/changes/cr-22-result-slots/tasks.md
- openspec/changes/cr-26-pg-lease-queue/tasks.md
- openspec/changes/cr-27-pg-mailbox/tasks.md
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-30-pgmq-sql-backend/tasks.md
- openspec/changes/cr-34-mailbox-composition/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-36-pg-matrix/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
