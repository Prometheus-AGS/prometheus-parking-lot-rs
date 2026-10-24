---
type: SessionRecord
id: karpathy-session-b9e44a601265f818
title: Karpathy session b9e44a601265
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T14:26:14.642512+00:00
created_at: 2026-10-23T14:26:14.642512+00:00
updated_at: 2026-10-23T14:26:14.642512+00:00
revision: 0
---

## Delta

Both round-10 changes are done and open as pull requests. CI is still running on both, so please wait for my confirmation before merging. `main` was green after round 9.

**PR #36 (cr-36): Postgres tests now run in CI on versions 17 and 18.** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/36
- **CI job:**
  - Builds the project's Postgres image.
  - Runs every Postgres test that needs Docker. CI has skipped all of these until now.
  - This PR's own CI is the job's first real run.
- **Autovacuum:** every queue table now gets cleaned on a fixed schedule rather than a percentage of its size. That stops dead rows piling up when a backlog is large. The numbers are reasonable starting points, not measured.
- **Postgres 18 runbook:** cites the official docs. It covers checksums, which are on by default in 18 and off in 17, the new data path in the 18 Docker image, password authentication and upgrading from 17.
- **Locally:** 66 of 66 Postgres tests passed on 17 and on 18. I also ran the new autovacuum test with the tuning deliberately removed, and it failed as it should.

**PR #37 (cr-49): accepted work is no longer lost if the process crashes.** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/37
- **How a durable pool handles each task:**
  - It saves the task before telling the caller it was accepted.
  - It takes the task from the queue under a lease and renews that lease while the task runs.
  - It writes the result and only then marks the task done.
- **Main crash test:** the process is killed in the middle of a task, and again after a task finishes but before its result is saved. After a restart, the task completes and exactly one result is readable. This passed on both Postgres 18 and redb.
- **Negative control:** with the task marked done before its result is saved, the result is lost and the test catches it.
- **Config:** a pool can now be pointed at a Postgres or redb backend, and the example config files run durably.

What "never lost" means precisely:
- Each task produces exactly one result.
- The task itself can run more than once. That happens if the process dies after the task starts but before it is marked done, or if the process stalls longer than the lease.
- So tasks with outside side effects, such as sending email or charging a card, must be safe to repeat. This is documented.

Known gaps in cr-49:
- Only the async pool has a durable mode. The threaded worker pool doesn't.
- An idle durable pool checks for new work every 500 ms instead of being notified by Postgres.
- There's no formal concurrency model of the new dispatch loop, though it reuses the dispatcher that already has one.
- The config builder's signature changed, which is a breaking change.

I added one fix of my own to #37: a code comment claimed the older queue interfaces would be removed by this change, but the in-memory pool still uses them.

**Merge order:** #36 first, then #37. After #36 merges I'll update #37 so its Postgres crash tests also run in the new CI job. Without that update they'd be skipped, because they currently look for Postgres 18 by a hard-coded name and aren't in the new job's test list.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T14:26:07.780566Z
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
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
