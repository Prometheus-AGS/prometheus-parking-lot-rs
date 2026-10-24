---
type: SessionRecord
id: karpathy-session-2f973364f77c031d
title: Karpathy session 2f973364f77c
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T21:35:49.881327+00:00
created_at: 2026-10-23T21:35:49.881327+00:00
updated_at: 2026-10-23T21:35:49.881327+00:00
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
- Captured: 2026-10-23T21:19:44.956899Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
