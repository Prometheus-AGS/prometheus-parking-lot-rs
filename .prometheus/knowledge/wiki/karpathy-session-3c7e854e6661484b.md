---
type: SessionRecord
id: karpathy-session-3c7e854e6661484b
title: Karpathy session 3c7e854e6661
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T16:25:10.907329+00:00
created_at: 2026-10-23T16:25:10.907329+00:00
updated_at: 2026-10-23T16:25:10.907329+00:00
revision: 0
---

## Delta

cr-31 (lifecycle events) is up as PR #42, with CI still running. #41 (the docs refresh) is already merged: CI on `main` passed, and the updated site is deployed. Please wait for my confirmation that #42's checks pass before merging it.

**PR #42:** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/42

Every pool except the WASM one now reports what happens to each task:
- **Event order:** each task reports "submitted", then "queued" if it had to wait, then "started" if it ran, then exactly one final event. The final event is one of: completed, failed, rejected, cancelled, expired, or requeued.
- **Hooks can't slow scheduling:** hooks run on a separate background thread. If a hook is slow, blocks, or crashes, scheduling carries on, and the events it can't keep up with are dropped and counted.
- **No hooks, no cost:** with no hooks registered, emitting an event costs well under a nanosecond.
- **With one hook:** about 136 ns per event. A 100-task benchmark runs about 64% slower.
- **Docs:** I updated the events page on the site, which still said the pools emit nothing, and merged in the latest `main` (#40 and #41).

**Tests (all run locally):**
- The six event-order tests pass. They cover:
  - cancellation under each shutdown policy;
  - durable tasks put back on the queue;
  - rejections at submit;
  - a 1,000-task concurrent run with no dropped events.
- The negative control, which removes the final event when a task is cancelled, fails exactly the two cancellation tests, as intended.
- The shutdown tests (19/19) and the crash tests (4/4) still pass.
- The full test suite (305 passed) passes after the merge.

**Breaking changes:** registering a hook no longer needs a mutable bus, and `HookBus::emit` now delivers in the background instead of immediately.

**Gaps:**
- If hooks fall far behind, an observer can miss a task's final event. That is the trade-off for never slowing scheduling.
- The new config-builder entry point that attaches hooks has no test yet.
- For durable pools, events were tested on the in-memory backends only, not Postgres or redb.
- When expired tasks are cleared out of the queue, no event is sent for them. Fixing that needs a change to the queue interface, which is tech-lead's area.
- The WASM pool doesn't emit events yet. That is planned for cr-53.

**Next:** once #42 merges, round 13 starts: cr-20 (skip-ahead with a starvation guard) and cr-32 (Iggy events), in parallel.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T16:19:26.542363Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
