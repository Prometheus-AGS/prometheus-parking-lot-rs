---
type: SessionRecord
id: karpathy-session-d10b626a82d3dd61
title: Karpathy session d10b626a82d3
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-22T16:39:32.435419+00:00
created_at: 2026-10-22T16:39:32.435419+00:00
updated_at: 2026-10-22T16:39:32.435419+00:00
revision: 0
---

## Delta

The core contracts change, `cr-18`, is complete except for one merge conflict, which I've sent back to the tech-lead.

**What `cr-18` delivers** (checked by the agent; I'll re-run the gates after the merge):
- **Fallible executors:** both executor traits return `Result<T, TaskError>`. `TaskError` separates executor failures (permanent or retryable), panics, timeouts, cancellation and expired deadlines.
- **One error family:** `SchedulerError` replaces `PoolError` everywhere, with a documented mapping. `TaskStatus::Failed` now carries the typed error.
- **Testable time:** a `Clock` trait with `SystemClock` and a deterministic `TestClock`. `submit` and `prune_expired` no longer take a timestamp.
- **Ordering:** a monotonic submission sequence makes equal-priority tasks run in submission order. A 1,000-task test checks this.
- **Resource-vector cost:** `ResourceCost` becomes a vector of named resources. It still reads the old JSON form, so `cr-23` only changes admission, not types.
- **ResourcePool rework:** one shared `Arc<Inner>`, and capacity is claimed only under the queue lock with overflow-checked arithmetic. Tasks costing more than the pool's total are rejected with `CapacityExceeded`. Expired queued tasks never run, and `shutdown()` rejects new work.
- **Cleanup:** `sync_wake_worker_loop`, the function that discarded tasks, is deleted. Lint suppressions drop from 30 to 20. Tests go from 64 to 79. The decision record is ADR 0002, and the agent drafted the 0.3.0 changelog text.

**The conflict:** `cr-18` was branched before the timeout fix (#13), and both changed `worker_pool/native.rs`. The tech-lead is resolving it to keep #13's deadline behaviour while using `cr-18`'s new error and result types. They'll then re-run the gates and the timeout tests ten times.

**Gaps to know about:**
- The WASM pool edits haven't been compiled for wasm32 yet. `cr-38` does that, and also needs a WASM-safe clock.
- The lock-based protection against a new task jumping the queue holds by construction, but no test can trigger the race yet. The concurrency models in `cr-19` will prove it.
- The old, never-compiled test suites were only partly updated; `cr-35` owns them.

**Still waiting on:** `main`'s first CI run on the new workflow (queued for a runner), and `cr-45` (docs site).

**KBD status:** `phase-correctness-and-durability › execute`, 3/37 complete. In progress: `cr-18` (merge being resolved), `cr-45`, `cr-42` (waiting on CI evidence). Blocked: `cr-15` (deferred).

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: unknown
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-22T16:39:22.729218Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
