---
type: SessionRecord
id: karpathy-session-770a3a672d1b5da7
title: Karpathy session 770a3a672d1b
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T11:51:22.987637+00:00
created_at: 2026-10-23T11:51:22.987637+00:00
updated_at: 2026-10-23T11:51:22.987637+00:00
revision: 0
---

## Delta

cr-27 is finished, and its PR is open. CI is running now. Don't merge it until I confirm the checks pass.

**PR #31 (cr-27, Postgres result mailbox):** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/31

**Correction:** the repository is `prometheus-parking-lot-rs`, not `prometheus-parking-lot`. The links I gave you earlier for #28–#30 were missing the `-rs`. GitHub probably redirected them, since you merged all three.

**What the PR does:**
- **New Postgres result mailbox.** It uses sqlx directly, with no ORM and no blocking calls. Each result is delivered to exactly one consumer, and expired results are never returned. When the same key is delivered twice, the first write is kept.
- **sea-orm and sqlx 0.8 removed.** Neither remains anywhere in the dependency graph.
- **Old Postgres queue and mailbox deleted.** The new lease queue from cr-26 replaces the old queue, so it wasn't worth porting. Old databases keep their existing tables; nothing drops them.

**What I checked locally:**
- All 15 mailbox tests pass against the Postgres 18 image, including a test where two consumers race for the same result.
- I also ran the deliberately broken version of the delete step, where the read and the delete are separate statements. That race test then fails with two consumers getting the same result, which shows the test catches the bug it is meant to catch.
- Every other check passes, including the whole-workspace test run and the dependency/license audit.

**Not yet tested:**
- Postgres 17; cr-36 adds it to CI.
- The example program against a live database.
- The Postgres tests also need Docker, so CI skips them until cr-36.

**One deviation from my instructions:** the agent edited the top-level crate's re-exports in `src/lib.rs`, which I had asked it not to touch. It had to, because those lines pointed at the deleted types and nothing would build otherwise. I checked the change and it is limited to that.

**On top of the agent's work, I:**
- added the CHANGELOG entry;
- updated the README, `docs/API.md` and `docs/agent-context.md`;
- fixed a stale doc comment.

A few docs still describe the old Postgres backends and the removed file-based Yaque queue: the website concept pages, `WHAT_WHY.md` and `ARCHITECTURE.md`. I've left those for the docs pass.

**Still running:** tech-lead on cr-37 (config schema). It changes the same two top-level files as cr-27, `src/lib.rs` and the root `Cargo.toml`. Whichever PR merges second will need a small update first.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: unknown
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T11:51:17.578469Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- .kbd-orchestrator/current-waypoint.json
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
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
