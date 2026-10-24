---
type: SessionRecord
id: karpathy-session-65f1853732314928
title: Karpathy session 65f185373231
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T21:49:27.376056+00:00
created_at: 2026-10-23T21:49:27.376056+00:00
updated_at: 2026-10-23T21:49:27.376056+00:00
revision: 0
---

## Delta

All four reviews are back, and between them they found 6 critical issues. The release stays blocked until each one is fixed and re-checked.

| Area reviewed | Critical | Warnings | Status |
|---|---|---|---|
| Postgres and redb backends | 2 | 4 | **redb fixes done, PR #74** (CI running). pgmq/Postgres fixes in progress. |
| Core admission and interfaces | 0 | 6 | scheduler-engineer fixing |
| Runtime pools and durability | 1 | 7 | tech-lead fixing |
| Config, Iggy, bindings, WASM, release readiness | 3 | 10 | platform-engineer (packaging); events-engineer (Iggy and config, queued after its current task) |

**All six critical issues:**
1. **pgmq can lose a requeued task.** After a delayed requeue, the old lease token still works, so a stale ack deletes the task.
2. **One unreadable task stopped the redb queue for good.** *Fixed in #74.*
3. **"Exactly one result" depended on the pool's name.** Workers with different names can each store a result for the same task.
4. **The embedded crate can't be packaged for publishing.** It lists itself as a versioned dev-dependency, so neither it nor the main crate can be published.
5. **The version is still 0.2.0, while all docs say 0.3.0.**
6. **An Iggy access token in a connection string is printed in full** whenever the config is logged with `Debug`.

**PR #74 (redb fixes):** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/74
- **Unreadable tasks** now go to the dead-letter queue instead of blocking every later lease.
- **New retry limit:** `max_attempts` is a new setting, also available in config.
- **Unreadable stored results** are now deleted and reported once.

Each of these was reproduced with a failing test before it was fixed. Please wait for CI to pass before merging.

**Other notable warnings being fixed:**
- **Iggy event IDs collide** when tasks reuse an ID. The README examples use `id: 0` for everything, and the colliding events are dropped as "duplicates".
- **Stream replay can wait forever** if the topic's real partition count differs from the configured one.
- **Iggy topics never expire.** There's no retention setting, so disk use only grows.
- **A Postgres password passed as a URL query parameter isn't masked.**
- **Two config sections can point at the same physical queue**, which gets around the "one pool per backend" rule.
- **The main crate doesn't build for WASM with default features**, even though the docs say it does.
- **The six library crates ship without a README or a license file.**
- **`PUBLISHING.md` is wrong:** it gives the old minimum Rust version and a publish command that can't work.

**What happens next:**
1. Each fix branch becomes a PR for you to merge, in an order I'll give you, because some touch the same files.
2. I run a short re-review of just the fixes.
3. I rerun the full gate while no agents are working.
4. If that passes, I run the 0.3.0 release (cr-52), which you've already approved.

Smaller suggestions that don't make it in will be filed as 0.4.0 issues.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: unknown
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T21:37:45.179351Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- No changed paths detected.
