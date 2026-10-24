---
type: SessionRecord
id: karpathy-session-15eb299b4686db05
title: Karpathy session 15eb299b4686
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T21:02:56.385898+00:00
created_at: 2026-10-23T21:02:56.385898+00:00
updated_at: 2026-10-23T21:02:56.385898+00:00
revision: 0
---

## Delta

PR #72 is open with the three API fixes from tech-lead. CI is running now, so please wait for my confirmation before merging. It's the last change before the phase gate: https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/72

**What it fixes:**
1. **Notifier types exported from the main crate.** `BackgroundNotifier` and the types that go with it now come from `prometheus_parking_lot`. You can attach the Iggy notifier without depending on the internal core crate. The Iggy example now uses only the main crate.
2. **Iggy stream topic in config.** The Iggy config section has a `stream_topic` setting, defaulting to `stream.chunks`.
3. **Clearer capacity errors.** When a task can never fit, the error now names the pool as you configured it and the resource that ran out, for example "need 30 gpu_vram units, pool 'inference' has 24". Lifecycle events from config-built pools carry the configured name too.

**Checks run locally:**
- All 352 tests pass.
- The config tests pass against real Postgres.
- I ran the three affected examples, two of them against a real Iggy server, and all exited cleanly.
- Every other check passes too.

**Breaking changes:** the capacity error has a new shape, and code that builds the Iggy config struct directly must now set the new field. Both are in the CHANGELOG.

I also updated the cr-47 design note: each API gap is now marked either fixed or linked to its 0.4.0 issue.

**After #72 merges, the phase gate starts:**
1. A full integration run against every real backend.
2. An independent review of every change in the phase.
3. If both pass, the 0.3.0 release, which you've already approved.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T21:02:55.475205Z
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
