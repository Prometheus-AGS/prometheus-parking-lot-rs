---
type: SessionRecord
id: karpathy-session-95a21701e5a2ebe5
title: Karpathy session 95a21701e5a2
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T12:00:48.299598+00:00
created_at: 2026-10-23T12:00:48.299598+00:00
updated_at: 2026-10-23T12:00:48.299598+00:00
revision: 0
---

## Delta

cr-37 (the config schema) is up as PR #32, now built on top of cr-27. Its CI is still running, so wait until I confirm the checks pass before merging. It's the last PR in round 8.

**PR #32:** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/32

**Since the first pass:**
- The config-built Postgres backend now includes the new result mailbox. Both share one connection pool, and `migrate` applies both database migrations.
- The Postgres config test now stores a result and reads it back once against Postgres 18. Reading it a second time returns nothing.
- The crate builds with every feature combination I tried: no default features, default, Postgres only, embedded storage only, and both together.
  - Two of those failed before because a benchmark needed the Tokio runtime feature to build. That benchmark now declares that requirement.
- `WHAT_WHY.md` now says pools run in memory, and that switching a pool to a durable backend by config alone waits on cr-49.
- The secret scan of the branch found nothing. The example Postgres URL has no password.

**Checks:** everything passes locally at `9b00265`: the config tests (22, including Postgres 18) and the full set of workspace checks (246 passed, 40 skipped).

**Not rechecked since the cr-27 merge:** the negative control, where the config is deliberately made to accept unknown fields. It earlier made exactly the six misspelled-field tests fail, and the merge didn't touch the code it controls.

**Coming up:** once #32 merges and `main` is green, I'll close cr-37's tasks in KBD and start round 9:
- **cr-30:** a Postgres backend using the pgmq extension through SQL.
- **cr-34:** a mailbox that combines storage with a notifier.
- **cr-44:** I'll confirm its scope from the plan before dispatching.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T12:00:43.315790Z
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
- openspec/changes/cr-38-wasm-target/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
