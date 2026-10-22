---
type: SessionRecord
id: karpathy-session-40f70a51b06dcbd9
title: Karpathy session 40f70a51b06d
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-22T16:12:45.118677+00:00
created_at: 2026-10-22T16:12:45.118677+00:00
updated_at: 2026-10-22T16:12:45.118677+00:00
revision: 0
---

## Delta

The workspace split, `cr-48`, is done and up as PR #11: https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/11. That puts the phase at 3 of 37 changes.

**What the split produced:**
- **Six packages:** the root facade `prometheus_parking_lot`, plus `prometheus-parking-lot-core`, `-runtime-tokio`, `-pg`, `-embedded` and `-wasm`. All share one workspace version, edition, minimum Rust version and lint set. Each has its own list of files to publish.
- **A tokio-free core:** `cargo tree` shows no tokio, sqlx, sea-orm, redb or reqwest in the core crate. The core holds the contracts, config, hooks and in-memory mailbox, plus empty `dispatch/` and `semaphore/` modules for the scheduler engineer.
- **The decision record:** `docs/adr/0001-workspace-split.md`.

**Verification:** I re-ran the checks myself: clippy is clean across the workspace and the 1.95 check passes. 64 tests pass and 0 fail, exactly the same tests as before the split.

**Changes users will notice:**
- the yaque types now need the `embedded` feature and the Postgres types need `postgres`;
- the always-failing Postgres stub constructors are gone;
- logging targets now carry the new crate names.

**Found and routed, not fixed:** the flaky `test_timeout_handling` failed 2 of 8 runs. Its cause is now known: `retrieve_async` races two timeouts and returns the wrong error when the inner one finishes first. It goes to the scheduler engineer with the dispatcher work.

**Once #11 merges:** `cr-42` (CI rebuild), `cr-45` (docs site with GitHub Pages) and `cr-18` (core contracts) run in parallel. Each is owned by a different role, in separate files and separate worktrees.

**KBD status:** `phase-correctness-and-durability › execute`, 3/37 complete (`cr-17`, `cr-41`, `cr-48`). Waiting on your merge of #11. Blocked: `cr-15` (deferred).

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-22T16:12:17.136743Z
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
- openspec/changes/cr-41-toolchain-baseline/tasks.md
- openspec/changes/cr-48-workspace-split/tasks.md
