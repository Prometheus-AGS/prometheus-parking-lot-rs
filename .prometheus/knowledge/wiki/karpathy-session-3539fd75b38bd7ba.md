---
type: SessionRecord
id: karpathy-session-3539fd75b38bd7ba
title: Karpathy session 3539fd75b38b
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T13:13:13.433531+00:00
created_at: 2026-10-23T13:13:13.433531+00:00
updated_at: 2026-10-23T13:13:13.433531+00:00
revision: 0
---

## Delta

All three round-9 changes are finished and open as PRs. CI is still running on each, so don't merge yet; I'll tell you when they pass.

| PR | Change | What it adds |
|---|---|---|
| [#33](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/33) | cr-30 | A queue on the pgmq Postgres extension, called through plain SQL with one pgmq queue per priority |
| [#34](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/34) | cr-34 | A mailbox that stores each result first, then notifies in the background with a limited number of retries |
| [#35](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/35) | cr-44 | Android and iOS bindings, a new CI workflow for those platforms, and a Tauri desktop example |

**Local results:**
- **cr-30 (#33):**
  - 26 tests pass on both Postgres 18 and Postgres 17.
  - The negative control (all priorities in one queue) fails exactly the two priority-order tests.
  - The one compromise: to reject a stale lease, it reads pgmq's own queue table directly, because pgmq's functions have no way to do that check.
- **cr-34 (#34):**
  - With a notifier that always fails, every result is still stored and can be read back.
  - A slow notifier doesn't delay delivery.
  - The negative control (notify before storing) loses results, as intended.
- **cr-44 (#35):**
  - The Android libraries build for both chip architectures, and the iOS libraries build for device and simulator.
  - The generated Kotlin and Swift bindings run against the real library.
  - Queued work survives a restart, including a hard kill of the process.
  - UniFFI builds on Rust 1.95, so the minimum supported version stays where it is.

**Things to know before merging:**
1. **#35's new Platform workflow has never run on GitHub.** It is set up so it doesn't run on every PR, and it may need fixes on its first run. Don't make its jobs required checks: they only run when certain files change, so a required one would sit pending forever on PRs that don't trigger it.
2. **#35 also widens the existing license and advisory check (`cargo deny`) to cover every crate in the workspace.** Before this, it only checked the top-level crate's dependencies.
3. **Not tested on real targets:**
   - an Android emulator or device;
   - an iOS app running in Xcode;
   - the Tauri app window itself;
   - Tauri builds on Linux or Windows.
4. **Merge order:** #33, then #34, then #35. All three add a CHANGELOG entry, so the second and third may need a small conflict fix. I'll handle that if GitHub reports a conflict.

**On top of the agents' work, I added** the new bindings crate to the README's crate table and a short amendment to the workspace-layout decision record (ADR 0001).

**Follow-ups I'm tracking:**
- A pgmq entry in the config schema, for tech-lead.
- A Tokio-based timer and spawner for the background notifier, for scheduler-engineer.
- An optional `result_ttl_ms` config setting.
- Postgres autovacuum tuning for the pgmq tables, which is part of cr-36.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T13:13:08.618802Z
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
- openspec/changes/cr-16-docs-truth/tasks.md
- openspec/changes/cr-21-panic-containment/tasks.md
- openspec/changes/cr-22-result-slots/tasks.md
- openspec/changes/cr-26-pg-lease-queue/tasks.md
- openspec/changes/cr-27-pg-mailbox/tasks.md
- openspec/changes/cr-28-embedded-redb-queue/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
