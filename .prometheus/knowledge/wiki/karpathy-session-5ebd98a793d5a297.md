---
type: SessionRecord
id: karpathy-session-5ebd98a793d5a297
title: Karpathy session 5ebd98a793d5
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-24T02:08:26.187043+00:00
created_at: 2026-10-24T02:08:26.187043+00:00
updated_at: 2026-10-24T02:08:26.187043+00:00
revision: 0
---

## Delta

The runtime branch is done.

**Fixed, each reproduced first with a test that failed:**
- **Oversized tasks cycling forever.** Before the fix, a task that no consumer could fit was handed back 558 times in 2 minutes with no end. Now, after 5 hand-backs (configurable), the client gets a final "dropped" result and the task moves to the dead-letter queue. A consumer with enough capacity still runs it.
- **A crash racing shutdown is reported as failed** rather than requeued.
- **Submits that failed before writing anything** now return a new `NotStored` error and emit a final "rejected" event. Before, they emitted no final event at all.
- **The core hooks docs** now match the durable pools' event rules.
- **The long-wait test** now waits for actual events instead of a fixed time.

**One suggestion refuted, with evidence.** The reviewer's suggested fix for the builder methods panicking too early would have made them panic always, so the docs were corrected instead.

**One gap the agent flagged:** the redb queue doesn't use the new `NotStored` error yet. It's a one-line change per failure point, and I'll make it while combining the branches, because the new error type only exists on this branch.

**Tests run:** 400 workspace tests, plus the Postgres 18 crash, shutdown and full scheduler suites. They all pass.

Only scheduler-engineer is still running, on the loom CI job. When it's done, I'll combine all four round-2 branches into one PR.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-24T01:45:51.694851Z
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
- .prometheus/decisions.md
- .prometheus/gotchas.md
- openspec/changes/cr-16-docs-truth/tasks.md
- openspec/changes/cr-20-skip-ahead/tasks.md
- openspec/changes/cr-21-panic-containment/tasks.md
- openspec/changes/cr-22-result-slots/tasks.md
- openspec/changes/cr-23-multi-resource/tasks.md
- openspec/changes/cr-26-pg-lease-queue/tasks.md
- openspec/changes/cr-27-pg-mailbox/tasks.md
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-29-shutdown-cancellation/tasks.md
- openspec/changes/cr-30-pgmq-sql-backend/tasks.md
- openspec/changes/cr-31-hook-emission/tasks.md
- openspec/changes/cr-32-iggy-events/tasks.md
- openspec/changes/cr-33-stream-replay/tasks.md
- openspec/changes/cr-34-mailbox-composition/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-36-pg-matrix/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-39-dependency-hygiene/tasks.md
- openspec/changes/cr-40-benchmarks/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-47-docs-guides/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- openspec/changes/cr-53-wasm-parity/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
