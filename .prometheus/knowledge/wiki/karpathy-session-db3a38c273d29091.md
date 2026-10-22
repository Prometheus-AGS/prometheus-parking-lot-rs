---
type: SessionRecord
id: karpathy-session-db3a38c273d29091
title: Karpathy session db3a38c273d2
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-22T15:41:33.788068+00:00
created_at: 2026-10-22T15:41:33.788068+00:00
updated_at: 2026-10-22T15:41:33.788068+00:00
revision: 0
---

## Delta

Starting change 4 of 37: cr-48-workspace-split

With #9 and #10 merged, `main` has the 1.99.1 toolchain baseline and the GitHub Pages scope. The workspace split, `cr-48`, is now dispatched to the tech-lead.

It runs alone because it moves nearly every file. Anything running alongside it would have to rebase across the moves. The tech-lead is building:

- **`prometheus-parking-lot-core`** with no tokio and no IO: the core contracts, hooks, in-memory mailbox and queue, and empty `dispatch/` and `semaphore/` modules for the scheduler engineer to fill.
- **`-runtime-tokio`** (ResourcePool, native WorkerPool, shutdown, cancellation), **`-pg`**, **`-embedded`** and **`-wasm`**.
- **The root package `prometheus_parking_lot`**, which keeps its published name, holds config and builders, and re-exports the rest behind features.

It also adds:
- a decision record for the split;
- a per-crate list of files to publish, replacing the old `exclude` list;
- updated constraint checks, including a check that `cargo tree` shows no tokio, sqlx, sea-orm, redb or reqwest in the core.

The tests that ran before the split must all still run.

**Local merge cleanup:** pulling `main` collided with my uncommitted local copies of the KBD and knowledge files. I resolved it by keeping the local copies, which the KBD runtime treats as current, and the runtime state is intact.

**When the split lands:** the docs site with Pages (`cr-45`), the CI rebuild (`cr-42`) and the core contracts (`cr-18`) can run in parallel.

**KBD status:** `phase-correctness-and-durability › execute`, revision 240, 2/37 complete. In progress: `cr-48` (task 1 of 4). Blocked: `cr-15` (deferred).

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: unknown
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-22T15:41:32.460149Z
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
- openspec/changes/cr-41-toolchain-baseline/tasks.md
