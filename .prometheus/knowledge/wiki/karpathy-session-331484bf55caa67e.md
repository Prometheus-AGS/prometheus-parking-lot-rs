---
type: SessionRecord
id: karpathy-session-331484bf55caa67e
title: Karpathy session 331484bf55ca
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T20:41:34.604898+00:00
created_at: 2026-10-23T20:41:34.604898+00:00
updated_at: 2026-10-23T20:41:34.604898+00:00
revision: 0
---

## Delta

cr-47 (guides) is open as PR #50: https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/50. CI is still running, so please wait for my confirmation before merging. This run is also the first for the new docs-examples workflow. cr-40 (benchmarks) is the last change still being built.

**Guides:** there are nine new guides on the docs site:
- GPU inference pool
- Tauri desktop app
- Cloud Postgres with multiple tenants
- Reconnecting clients
- Shutdown and requeue
- Iggy events
- Streaming replay
- Configuration
- Upgrading from 0.2

Seven are backed by example programs, eight in all. I ran every one locally against real Postgres 18 and Iggy servers, and all exited cleanly. The configuration guide's JSON blocks are loaded by an existing test, and the upgrade guide is prose taken from the CHANGELOG.

**Guarding the guides in CI:** a new workflow builds and runs all eight examples on every change. A new check also fails CI if any Rust code in the guides drifts from the example file it was copied from. I made a deliberate edit and it caught it.

**API reference:** the docs site links to docs.rs instead of hosting its own API reference. Those links will only work once 0.3.0 is published.

**Library gaps the guides exposed** (reported, not fixed):
1. **Release blocker in my view:** `BackgroundNotifier` isn't exported from the main crate. Users who depend only on `prometheus_parking_lot` can't attach the Iggy notifier without also adding the internal core crate. This is a one-line fix.
2. A pool built from a config file can't take a notifier, result TTL, or heartbeat and poll timings. It also checks the queue on a timer instead of using Postgres notifications.
3. The Iggy config section has no field for the stream topic.
4. When a config-built pool rejects a task as too large, the error calls the pool "resource_pool" instead of its configured name, and doesn't say which resource ran out.
5. A config file with no pools, for example one that only sets up Iggy events, can't be loaded.

My plan: once cr-40 is in, I'll give tech-lead a small batch for gaps 1, 3 and 4 before the phase gate. Gaps 2 and 5 are design changes, so I'll leave them as documented limitations for 0.3.0 unless you want them in this release.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T20:41:32.558850Z
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
- openspec/changes/cr-49-durable-dispatch/tasks.md
- openspec/changes/cr-53-wasm-parity/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
