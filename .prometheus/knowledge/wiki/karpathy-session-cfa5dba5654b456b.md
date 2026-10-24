---
type: SessionRecord
id: karpathy-session-cfa5dba5654b456b
title: Karpathy session cfa5dba5654b
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T20:44:26.871590+00:00
created_at: 2026-10-23T20:44:26.871590+00:00
updated_at: 2026-10-23T20:44:26.871590+00:00
revision: 0
---

## Delta

My recommendation is to file each gap as a GitHub issue and let the next phase pick them up, rather than opening a child phase.

**Why not a child phase:**
- This phase is at its finish line: one change, cr-40, is still running, then come the phase gate and the release you approved.
- A child phase would reopen implementation here, push the gate and release back, and wrap one coherent release in a list of unrelated follow-ups.
- Child phases are for splitting up the work a phase set out to do. These gaps are findings that came out of the work, not part of the original scope.

**Why issues:**
- **Users can see them.** This becomes a public crate at 0.3.0. Issues show users the known limitations, and the CHANGELOG and README can link to them.
- **They outlast this phase.** An issue stays open until someone fixes it, whichever phase or tool does the work. KBD notes are tied to the phase they came from.
- **The tooling already supports it.** The agent team's intake turns `team:parking-lot-core` issues into tasks for the right role. And when this phase closes, KBD writes a reflection that recommends the next phase. That reflection can group these issues into the next phase's goals, so the next phase is made from issues rather than a separate list.

**How I'd split them:**

| Bucket | Where it goes | Items |
|---|---|---|
| Must fix before 0.3.0 | In this phase (tech-lead is doing it now) | Export `BackgroundNotifier` from the main crate; add `stream_topic` to the Iggy config; make the "task too large" error name the configured pool |
| Real gaps in the product | Issues, then the next phase (proposed: a 0.4 "durable ergonomics" phase) | Config-built durable pools can't take a notifier, result TTL or heartbeat/poll settings; they also poll instead of using Postgres LISTEN; config can't be loaded without a pool; optional `result_ttl_ms`; no event when the queue drops expired tasks (needs an interface change); skip-ahead for durable pools; a durable `WorkerPool` |
| Proof and test hardening | Issues, labelled `testing` | No concurrency model for the shutdown ledger or the release path of resources with per-type limits; no headless-browser WASM run; no real Postgres 17→18 upgrade test; the Tauri app GUI never run |
| Docs housekeeping | Issues, or a small PR | Mark ADRs 0001 and 0003 as superseded; the CI step name for the WASM job is stale |

There are about 15 issues in total. Each would cite the PR that found it, and the "known limitations" section of the 0.3.0 CHANGELOG would link to them.

Filing issues posts to the public repository, so I'll wait for your go-ahead. Shall I file them with labels (`enhancement` / `testing` / `docs`, plus `team:parking-lot-core`) under a `0.4.0` milestone?

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T20:44:25.832423Z
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
