---
type: SessionRecord
id: karpathy-session-26b346f18ee1cbfd
title: Karpathy session 26b346f18ee1
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T17:33:39.291687+00:00
created_at: 2026-10-23T17:33:39.291687+00:00
updated_at: 2026-10-23T17:33:39.291687+00:00
revision: 0
---

## Delta

cr-20 (skip-ahead) is done and open as PR #43, with CI running now: https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/43. Please hold the merge until I confirm the checks pass.

**What it does:** when the next task in line is too big for the free capacity, a smaller task that fits starts first. After 8 such skips, capacity is held for the big task until it starts. The proof that it can't be skipped more than 8 times is written into the code docs. The only thing that can still jump ahead of it is a task with strictly higher priority, the same as before this change.

**Benchmark** (simulated, at about 95% load):

| | Before | With skip-ahead (default) |
|---|---|---|
| Capacity in use | 91.7% | 94.7% |
| Large tasks' typical wait | 47 ticks | 4 ticks |
| Large tasks' worst wait | 73 ticks | 11 ticks |

**Local checks:**
- The 8 starvation tests pass. With the guard switched off, three of them fail, which is what should happen.
- A new concurrency model test (loom) and the memory checker (Miri) are clean.
- The full test suite passes (322 tests), including the earlier lifecycle-event, shutdown, config and WASM tests.

**Decision for you: skip-ahead is on by default.** The agent chose that default; the plan doesn't say. It changes the order tasks run in for existing users, so it's listed in the CHANGELOG as a breaking change. Setting `max_head_skips: 0` restores strict first-in-line order. Should it stay on by default, or ship off so users opt in? I lean towards keeping it on, because the numbers above favour it and 0.3.0 is already a breaking release.

**Not covered yet:**
- Durable pools still run tasks strictly in order. Supporting skip-ahead there would need several tasks leased at once, plus new crash tests.
- The WASM pool uses the same code but has no skip-ahead test of its own. That moves to cr-53.
- Skips don't send a lifecycle event. They are counted on the pool instead, because a new event type would break the event-order guarantee from cr-31.
- Adding the new config field breaks code that builds those config structs literally.

**Still running:** cr-32 (Iggy events). That agent is also checking whether the Iggy client needs a newer minimum Rust version, as you asked.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T17:31:10.104001Z
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
- openspec/changes/cr-16-docs-truth/tasks.md
- openspec/changes/cr-21-panic-containment/tasks.md
- openspec/changes/cr-22-result-slots/tasks.md
- openspec/changes/cr-26-pg-lease-queue/tasks.md
- openspec/changes/cr-27-pg-mailbox/tasks.md
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-29-shutdown-cancellation/tasks.md
- openspec/changes/cr-30-pgmq-sql-backend/tasks.md
- openspec/changes/cr-31-hook-emission/tasks.md
- openspec/changes/cr-34-mailbox-composition/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-36-pg-matrix/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
