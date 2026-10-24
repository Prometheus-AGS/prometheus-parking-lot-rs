---
type: SessionRecord
id: karpathy-session-e9296b63b51dcd81
title: Karpathy session e9296b63b51d
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-24T01:23:37.177572+00:00
created_at: 2026-10-24T01:23:37.177572+00:00
updated_at: 2026-10-24T01:23:37.177572+00:00
revision: 0
---

## Delta

The first re-review is back, covering the backends and runtime pools. **No critical issues.**

**Original findings:**
- 17 of 20 are fully fixed.
- 2 are only partly fixed:
  - **Durable-pool event rule docs:** the docs for the "one final event per run" rule are right everywhere except the core hooks API reference, which still contradicts it.
  - **Timing-sensitive tests:** two of them now just wait longer, instead of waiting for an actual condition.
- 1 was left unchanged on purpose: a documentation-only point about lost wake-up notifications, which the docs now describe accurately.

**New problems introduced by the fixes:** 3 warnings and 6 smaller suggestions. The three warnings:
1. **A task too big for every worker is handed back forever.** The earlier fix stopped oversized tasks from being thrown away, so a worker without capacity now hands them back. But if no worker can ever run the task, it cycles endlessly and the client never gets a result. That happens by default, because queues have no retry limit unless one is set.
2. **A migration step can now fail startup.** One `COMMENT ON` statement in the migrations still takes a table lock every time they run. With the new 5-second lock timeout, a manual `VACUUM` running at the same moment makes startup fail instead of just waiting.
3. **Stale API reference.** The core hooks reference still says a durable pool emits "rejected" for a task that's too large; it now emits "requeued".

The second re-review covers core, Iggy, config and release readiness, and is still running. When it's back, I'll send both sets of new findings to the owning agents in one fix round, so everything raised gets fixed as you asked, and then run the final gate.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-24T01:23:11.005816Z
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
