---
type: SessionRecord
id: karpathy-session-238f149b1683234e
title: Karpathy session 238f149b1683
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-24T08:12:44.763957+00:00
created_at: 2026-10-24T08:12:44.763957+00:00
updated_at: 2026-10-24T08:12:44.763957+00:00
revision: 0
---

## Delta

No. Nothing known stops the crate working in normal use.

**On `main` today:** all CI checks pass, including the real Postgres 17/18, redb, Iggy, WASM, Android, iOS and Tauri jobs, and the guide examples run. Submitting tasks, admission, durable crash recovery, shutdown and events all work as documented. All 6 critical findings from the first review are fixed and merged.

**One critical on `main` is a security problem, not a functional one.** If an Iggy password contains `?`, it appears in full when the config is printed with `Debug`, for example in a log. Nothing stops working. It's fixed in the round-2 branch, which isn't merged yet. That's the main reason to merge round 2 before releasing.

**The rest of round 2 is edge cases.** For example:
- A task too large for every worker cycled forever.
- A failed submit that stored nothing sent no final event.
- A migration rerun timed out while a manual `VACUUM` was running.

None of these affects a normally configured deployment.

**One thing I haven't confirmed.** A Postgres 18 test run had 3 failures while the machine was heavily loaded, then passed 79 of 79 on rerun. I believe it was load, but that's unproven. The final gate on a quiet machine will settle it.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-24T08:12:39.345803Z
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
