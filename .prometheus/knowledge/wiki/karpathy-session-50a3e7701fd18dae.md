---
type: SessionRecord
id: karpathy-session-50a3e7701fd18dae
title: Karpathy session 50a3e7701fd1
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T16:40:05.780406+00:00
created_at: 2026-10-23T16:40:05.780406+00:00
updated_at: 2026-10-23T16:40:05.780406+00:00
revision: 0
---

## Delta

**Ready to merge:** PR #42 (cr-31, lifecycle events) passed all 18 CI checks and has no conflicts: https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/42

The checks it passed include the Postgres 17/18 jobs, the Android, iOS and Tauri jobs, and the docs-site build. The 19th entry, the GitHub Pages deploy, is skipped on PRs.

**On `main`** (at `e36fb84`): everything through #41 is merged, and CI and the docs-site deploy are green. Nothing is running in the background.

**Phase progress:** the work has been running in rounds of parallel agents.
- **Done:** rounds 1–11 plus the round-12 follow-ups (#40 and #41).
- **Merged in rounds 6–11:**
  - result slots, the WASM pool and the Postgres lease queue;
  - the Postgres mailbox and the config schema;
  - the pgmq queue, the composable mailbox and the mobile bindings;
  - the Postgres 17/18 CI matrix and durable dispatch;
  - shutdown and cancellation.
- **Round 12:** #42 is the last open item.

**After you merge #42:**

| Round | Changes |
|---|---|
| 13 | cr-20 (skip-ahead with a starvation guard) and cr-32 (Iggy events), in parallel |
| 14 | cr-23 (multi-resource costs), cr-39 (dependency cleanup), and cr-33 (stream replay, an optional extra) |
| 15 | cr-40 (benchmarks), cr-47 (docs guides) and cr-53 (WASM parity) |
| 16 | cr-52 (the 0.3.0 release), which needs your approval before anything is published |

Before the release, there is one end-of-phase gate: a full integration run and an independent review of all the changes. That review is what closes every change's review task, which is still open.

**Open items for you or the owning roles:**
- **tech-lead to confirm:** the reworded core rule in `docs/agent-context.md`. It is now "parking-lot-core has no runtime, IO, database or network dependencies".
- **Your decision, if you want it:** whether pgmq should be allowed to run a durable pool. Today it is rejected, because nothing has tested that pgmq survives a crash in that setup.
- **Still to route:**
  - the optional `result_ttl_ms` setting;
  - the change that sends an event when expired tasks are cleared;
  - "superseded" notes on ADRs 0001 and 0003.
- **Recurring:** an empty git `index.lock` left with no git process running. I've removed it twice now, and the cause is still unknown.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T16:40:03.560034Z
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
- openspec/changes/cr-16-docs-truth/tasks.md
- openspec/changes/cr-21-panic-containment/tasks.md
- openspec/changes/cr-22-result-slots/tasks.md
- openspec/changes/cr-26-pg-lease-queue/tasks.md
- openspec/changes/cr-27-pg-mailbox/tasks.md
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-29-shutdown-cancellation/tasks.md
- openspec/changes/cr-30-pgmq-sql-backend/tasks.md
- openspec/changes/cr-34-mailbox-composition/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-36-pg-matrix/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
