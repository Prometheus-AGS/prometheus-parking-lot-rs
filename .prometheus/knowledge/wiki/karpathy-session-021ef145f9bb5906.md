---
type: SessionRecord
id: karpathy-session-021ef145f9bb5906
title: Karpathy session 021ef145f9bb
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T20:43:33.131344+00:00
created_at: 2026-10-23T20:43:33.131344+00:00
updated_at: 2026-10-23T20:43:33.131344+00:00
revision: 0
---

## Delta

#50 (cr-47, the guides) is merged. I've synced `main` to `acea906` and closed cr-47's implementation tasks in KBD.

**CI on `main`:** #50 merged before most of its checks had run, including the first-ever run of the new job that builds and runs all eight guide examples. CI on `main` is running those checks now. If the examples job fails on GitHub's runners, I'll fix it before the phase gate.

**Two agents are running:**
- **cr-40 (benchmarks):** the last planned change, still in progress.
- **tech-lead:** started on three of the five API gaps the guides found, to land before the release:
  1. Export `BackgroundNotifier` and its related types from the main crate. The Iggy example then uses only `prometheus_parking_lot`.
  2. Add the missing `stream_topic` setting to the Iggy config section.
  3. Make the "task too large" error name the pool as configured instead of "resource_pool". Where it fits, it will also say which resource ran out.

The other two gaps are larger design changes: config-built durable pools can't take a notifier or use Postgres notifications, and a config file can't be loaded without at least one pool. Unless you want them in this release, I'll list them as known limitations of 0.3.0.

**After these two finish:** the phase gate runs, which is a full integration run and an independent review of every change. If it passes, the 0.3.0 release follows, which you've already approved.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T20:43:28.233229Z
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
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-47-docs-guides/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- openspec/changes/cr-53-wasm-parity/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
