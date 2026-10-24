---
type: SessionRecord
id: karpathy-session-6c6a5c27399b3c7e
title: Karpathy session 6c6a5c27399b
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T18:55:44.767144+00:00
created_at: 2026-10-23T18:55:44.767144+00:00
updated_at: 2026-10-23T18:55:44.767144+00:00
revision: 0
---

## Delta

cr-33 (stream replay) is done. Because my README message reached it after it finished, it has restarted to update the README to match. I'll open its PR once that update lands.

**cr-33:**
- **What it adds:** a client that disconnects mid-stream can reconnect and get exactly the chunks it missed, in order, then the end marker.
- **Protection against gaps and duplicates:** the reader tracks a sequence number for every chunk. A repeat is skipped. A missing chunk is reported as an error instead of being silently passed over.
- **When the send queue is full:** the executor waits for room instead of chunks being dropped, because a dropped chunk would leave a gap. If a chunk can't be sent even after retries, the stream stops there rather than continuing past the hole.
- **Tests:** the three new tests pass against the real Iggy server. They cover:
  - a reconnect partway through 200 tokens;
  - a retried chunk being delivered once;
  - a missing chunk being reported.

  The negative control, a reader that ignores sequence numbers, fails all three as it should. The full suite passes (334 tests).
- **Not tested:** the path where a stream stops after a chunk fails to send is checked by reasoning only, with no Docker test. Also, a client waiting on a stream that stopped must use its own timeout, because no end marker will ever arrive.

**README PR #45:** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/45. CI is still running, so please wait for my confirmation before merging. I checked every claim against the code on `main`. The main fixes:
- It now describes all four pools; the durable pool and the WASM pool were missing.
- New status rows for lifecycle events and result notifications.
- The pgmq queue appears in the crate and feature tables.
- The config section lists the pgmq and iggy backends and their example files.
- The WASM row no longer says it is "still to be built".
- Links to the docs site, the mobile bindings and the Postgres 18 runbook.

I left the "multi-resource" row alone, because cr-23 is changing that behaviour right now. I've told all three round-14 agents to update the README parts their change affects, after merging `main`, so each PR leaves it accurate.

**For you:** the cr-33 agent left a build folder from its negative-control run. Its attempt to delete it was denied, so I haven't removed it either. To clear it:
`! rm -rf /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/events-engineer/target-nc33`

**Still running:** cr-23 (multi-resource admission) and cr-39 (dependency cleanup).

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T18:55:24.425162Z
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
- openspec/changes/cr-26-pg-lease-queue/tasks.md
- openspec/changes/cr-27-pg-mailbox/tasks.md
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-29-shutdown-cancellation/tasks.md
- openspec/changes/cr-30-pgmq-sql-backend/tasks.md
- openspec/changes/cr-31-hook-emission/tasks.md
- openspec/changes/cr-32-iggy-events/tasks.md
- openspec/changes/cr-34-mailbox-composition/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-36-pg-matrix/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
