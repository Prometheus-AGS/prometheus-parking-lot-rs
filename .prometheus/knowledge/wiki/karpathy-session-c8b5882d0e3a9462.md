---
type: SessionRecord
id: karpathy-session-c8b5882d0e3a9462
title: Karpathy session c8b5882d0e3a
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T09:32:11.688155+00:00
created_at: 2026-10-23T09:32:11.688155+00:00
updated_at: 2026-10-23T09:32:11.688155+00:00
revision: 0
---

## Delta

Round 7 is fully up for review: all three changes have PRs, and CI is running on each. I'll confirm each one from its final job list before calling it ready to merge.

| Order | PR | Change | What it does |
|---|---|---|---|
| 1 | #28 | cr-22 result slots | Waiting for a result no longer ties up a thread: 10,000 waiters all complete on one blocking thread. Unclaimed results now expire after a set time, and a late retrieve reports `ResultExpired`. |
| 2 | #29 | cr-38 WASM target | The WASM pool now uses the shared dispatcher, so it finally respects `max_units` (audit C1). Time limits use JS timers. Adds a WASM CI job and a check that keeps tokio and databases out of the WASM build. |
| 3 | #30 | cr-26 Postgres lease queue | Async lease queue on raw sqlx 0.9. Tasks are leased with `SKIP LOCKED`, stale workers are fenced out, and idle dispatchers are woken by LISTEN/NOTIFY. It passes 20 of 20 Docker tests, including recovery after a SIGKILL and 8 concurrent consumers over 10,000 tasks. |

**What I checked myself:** I merged all three on top of `main` and ran every gate on the result:
- 233 tests pass; fmt and clippy are clean, natively and for WASM.
- The docs build without warnings, and the code compiles on Rust 1.95.
- The WASM dependency check, the orphan-test check and `cargo deny` all pass.
- I also wrote cr-26's CHANGELOG entries and confirmed they merge cleanly with cr-22's and cr-38's.

**Changes outside each agent's area, flagged in the PRs:**
- **cr-22** adds the `SchedulerError::ResultExpired` variant, which closes a spec gap.
- **cr-38** adds one attribute to a core config function, so WASM clippy runs without exceptions.
- **cr-26** moves the workspace's sqlx dependency to 0.9. sea-orm keeps 0.8 until cr-27.

**Still not verified:**
- The new WASM CI job hasn't run on a real GitHub runner, and the WASM tests run in Node only, not a browser.
- The Postgres tests have run on PG 18 only. cr-36 adds the 17/18 matrix and the Docker CI job.

Merge in the order above; they also merge cleanly in any order. Once they're in, round 8 is cr-27 (Postgres mailbox, which also removes sea-orm and sqlx 0.8) and cr-37 (config schema).

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T09:32:03.245589Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- .kbd-orchestrator/current-waypoint.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/hooks-status.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/hooks.log.jsonl
- .kbd-orchestrator/phases/phase-correctness-and-durability/progress.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/tasks.md
- .kbd-orchestrator/position-reminder.txt
- .kbd-orchestrator/position.json
- openspec/changes/cr-16-docs-truth/tasks.md
- openspec/changes/cr-21-panic-containment/tasks.md
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
