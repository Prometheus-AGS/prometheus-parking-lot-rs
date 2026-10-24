---
type: SessionRecord
id: karpathy-session-6f7469843b05d4ea
title: Karpathy session 6f7469843b05
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T14:13:33.420183+00:00
created_at: 2026-10-23T14:13:33.420183+00:00
updated_at: 2026-10-23T14:13:33.420183+00:00
revision: 0
---

## Delta

Round 9 is finished and round 10 has started.

**Round 9:**
- **`main`:** synced to `5a87503`, which includes #33, #34 and #35. CI and the new Platform workflow were still running on the merge commit when I last looked, so `main` isn't confirmed green yet. I'll check before opening the round-10 PRs.
- **KBD:** the implementation tasks for cr-30, cr-34 and cr-44 are closed. Their review tasks stay open for the end-of-phase review.

**Cleanup on this machine:**
- **Stale git lock:** another empty `index.lock` had appeared, timestamped 07:04. No git process was running, so I removed it, as last time. Something keeps leaving it behind. I suspect a hook, but I haven't confirmed that.
- **Two hung shells:** these were from this session and had been stuck for about 23 hours, since round 1, on a python3 script in the tech-lead worktree. I stopped them. That also closed the original round-1 tech-lead agent, which had been held open by those shells.
- **A third old shell, not mine:** it belongs to a different Claude session working in `flint-core-infra`. I left it alone.

**Round 10 (two agents working in parallel):**

| Change | Agent | Work |
|---|---|---|
| cr-49 | scheduler-engineer | **Durable dispatch.** This is the change that makes the crate's main promise true, that accepted work is never silently lost (see below). |
| cr-36 | postgres-engineer | **Postgres tests in CI on versions 17 and 18.** None of the Docker-based Postgres tests run in CI today; this adds a job that runs them. It also adds autovacuum tuning for the busy queue tables, fixes a stale config comment, and writes a Postgres 18 operations runbook based on the official documentation. |

What cr-49 does:
- `submit` succeeds only after the task is saved to the durable queue.
- Workers take tasks from that queue under a time-limited lease.
- A task is marked done only after its result is written to the durable mailbox.
- Long tasks renew their lease periodically so it doesn't expire mid-run.
- After a crash, unfinished work is handed out again once its lease expires.
- Config can then point a pool at Postgres or the embedded redb store. Until now the config validator rejected that.

The test for cr-49 kills the pool process at two moments: mid-task, and after the task finishes but before its result is written. After a restart, exactly one result must be readable, on both Postgres and redb. In the negative control, the task is marked done before its result is written, and the test must then detect the lost result.

**Risk:**
- cr-49 is the largest and hardest change in the phase, so expect it to take longer.
- It may need small changes to the shared queue and mailbox interfaces. The agent will list any it makes.
- It may also hit a limitation in a storage backend. If it does, it will stop and report rather than edit those crates.
- I asked it to state exactly where a task can still run twice, so the "never lost" claim is precise.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: unknown
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T14:12:19.610239Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
