---
type: SessionRecord
id: karpathy-session-eb349b02413c3c93
title: Karpathy session eb349b02413c
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T19:56:55.614237+00:00
created_at: 2026-10-23T19:56:55.614237+00:00
updated_at: 2026-10-23T19:56:55.614237+00:00
revision: 0
---

## Delta

Round 14 is finished and round 15, the last implementation round, has started.

**Round 14:**
- **#48:** it had passed every CI check.
- **`main`:** now at `6824e83`.
- **KBD:** cr-23's two implementation tasks are closed.
- **Leftover build folders:** gone, thanks.

**Round 15 (three agents working in parallel):**

| Change | Agent | Work |
|---|---|---|
| cr-40 | scheduler-engineer | **Benchmarks.** Measures three things: how many tasks per second each pool handles, how long a task waits to start and to return a result (median and slow cases), and how much of its capacity a pool keeps busy (comparing strict ordering vs skip-ahead, and one resource type vs several). Adds a CI job that saves the results and reports regressions without failing builds on noisy runners. Rewrites the performance section of `ARCHITECTURE.md` from real measurements, replacing the old unmeasured 0.2 numbers. |
| cr-47 | product-manager | **Guides.** Covers the main use cases: a GPU inference pool, a Tauri desktop app, cloud Postgres with many tenants, clients collecting results after reconnecting, streaming replay, shutdown and requeue, Iggy events, configuration, and upgrade notes. Each guide is backed by an example program or code sample that CI compiles. Also refreshes the agentic-value chapter and links the API reference. |
| cr-53 | platform-engineer | **WASM parity.** Brings the WASM pool level with the native pools: admission checked for every resource type, skip-ahead, and the same lifecycle events. The same event-order tests run under WASM. It also documents that crash-durable pools don't apply in the browser, because there's no durable storage backend there. |

All three agents also update the README sections their change affects.

**What happens next:**
1. **Merge round 15:** you merge the three round-15 PRs once they're up.
2. **Phase gate:** an integration run through the real backends plus an independent review of every change in the phase. That review also closes the review task each change still has open.
3. **Release 0.3.0 (cr-52):** you've already approved it. It starts if the gate passes, and it will be the crates' first publication on crates.io.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T19:56:52.594084Z
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
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
