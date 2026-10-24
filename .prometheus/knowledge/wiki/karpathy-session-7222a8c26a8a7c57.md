---
type: SessionRecord
id: karpathy-session-7222a8c26a8a7c57
title: Karpathy session 7222a8c26a8a
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T15:41:37.605595+00:00
created_at: 2026-10-23T15:41:37.605595+00:00
updated_at: 2026-10-23T15:41:37.605595+00:00
revision: 0
---

## Delta

Round 11 is done and round 12 is underway.

**Round 11:**
- **#39 (cr-29, shutdown policies):** it had passed all 17 of its CI checks.
- **`main`:** synced to `d91401b`. Its own CI run for that merge is still going.
- **KBD:** cr-29's implementation tasks are closed.

**Round 12:** the only planned change in this round is cr-31. Every remaining change depends on it, directly or through another change. Two roles would otherwise sit idle, so I gave them the leftover follow-ups. Three agents are working now:

| Agent | Work |
|---|---|
| scheduler-engineer | **cr-31 (lifecycle events).** Every pool emits an event when a task is submitted, waits for capacity, starts, completes, fails, is cancelled, goes back on the queue or is rejected. When no hooks are registered this costs nothing. A slow or crashing hook can't stall scheduling. The test checks that each task emits "submitted", then "started", then exactly one final event. The negative control removes the final event when a task is cancelled, and the test must catch it. |
| tech-lead | **Follow-ups:** <br>• A pgmq backend type in the config. It also decides whether a pgmq queue can run a durable pool, or whether the config should reject that with a clear error.<br>• The lint failure in the embedded crate.<br>• Turning the two `ignore`d examples in `lib.rs` into compiled doc tests.<br>• A shorter path to import `ShutdownReport`. |
| product-manager | **Docs refresh.** The website concept pages, `ARCHITECTURE.md`, `WHAT_WHY.md` and others still describe the removed Yaque and sea-orm backends, and say durability is "not yet" there. It also builds the site to catch broken links. |

The `result_ttl_ms` config setting is still on hold. It touches the same crate cr-31 is editing.

**Coming next:** round 13 is cr-20 (smaller tasks can skip ahead of a large one, with a cap so the large one isn't starved) and cr-32 (Iggy events). Both depend on cr-31.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T15:40:53.111382Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
