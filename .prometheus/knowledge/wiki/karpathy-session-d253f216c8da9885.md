---
type: SessionRecord
id: karpathy-session-d253f216c8da9885
title: Karpathy session d253f216c8da
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T21:41:36.088958+00:00
created_at: 2026-10-23T21:41:36.088958+00:00
updated_at: 2026-10-23T21:41:36.088958+00:00
revision: 0
---

## Delta

The second gate run stopped at the same step as the first: 428 of 431 Docker tests passed and 3 failed. All three failures look caused by timing under load. Each also exposed a test that is too sensitive to timing, so each has been sent to the agent that owns it.

The run had to share the machine with four fix agents and three reviewers. The load average was above 400, many times what the machine can handle. None of these three tests failed on the first run at almost the same commit.

| Test | What happened | Who's fixing it |
|---|---|---|
| pgmq `leased_task_is_invisible` | The lease is rounded up to 1 second and expired before the test checked the queue, so the task looked visible again. | postgres-engineer (already working in pgmq) |
| `shutdown_durable_pg_drain_restart_runs_the_rest` | "Never reached its start state": a fixed wait was too short under load. | tech-lead (already working in the durable pool) |
| Iggy `lifecycle_events_arrive_in_order` | 7 messages arrived instead of 6. Probably a send timed out after it had already reached the server, and the retry stored a second copy. That duplicate is documented as possible. If so, the test should de-duplicate by message ID, as the docs tell consumers to. | events-engineer (after the redb fixes) |

Each agent has to confirm the cause before changing anything. If any of these turns out to be a real bug rather than a timing-sensitive test, it gets fixed as a bug.

**Change to how I run the final gate:** I'll wait until no agents or other builds are running, so the result reflects the code and not the machine's load. I've written that down in `.prometheus/gotchas.md`.

**Still running:**
- Four fix agents: pgmq/Postgres, redb, core, and runtime pools.
- The last review: config, Iggy, bindings, WASM and release readiness.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T21:26:39.597101Z
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
