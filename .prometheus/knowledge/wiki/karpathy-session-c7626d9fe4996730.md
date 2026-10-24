---
type: SessionRecord
id: karpathy-session-c7626d9fe4996730
title: Karpathy session c7626d9fe499
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T16:59:16.433341+00:00
created_at: 2026-10-23T16:59:16.433341+00:00
updated_at: 2026-10-23T16:59:16.433341+00:00
revision: 0
---

## Delta

Round 12 is finished and round 13 has started.

**Round 12 wrap-up:**
- **`main`:** synced to `564a1bd`, which includes #42.
- **KBD:** cr-31's two implementation tasks are closed.
- **Stale lock again:** another empty git lock file had appeared, this time at 11:25. Nothing was holding it, so I removed it. It has now happened three times. The likeliest cause is a program crashing partway through a git command while the repo is open in another tool, such as an editor running its own git status checks. I haven't confirmed that.

**Round 13 (two agents working in parallel):**

| Change | Agent | Work |
|---|---|---|
| cr-20 | scheduler-engineer | **Skip-ahead.** When the next task in line is too big for the free capacity, a smaller task that fits can go first. After a set number of skips, capacity is held back for the big task so it can't wait forever. The agent will state that limit precisely and prove it. It applies to every pool that uses the shared dispatcher, WASM included. For the durable pool it will either support skip-ahead or explain why not yet. Test: a large task starts within the limit even while small tasks keep arriving. Negative control: with the guard switched off, that test must fail. |
| cr-32 | events-engineer | **Iggy events.** A new `parking-lot-iggy` crate, behind an `iggy` feature, sends lifecycle events and "result ready" notices to an Iggy server. Message IDs are derived from the task's key, so a retry doesn't create a duplicate. Sending never happens on the scheduling path: if the server is slow or down, events buffer up to a limit and are then dropped and counted. Tests run against a pinned Iggy server in Docker: the same key yields one message ID, a subscriber can replay from an earlier point, and pausing the server doesn't slow dispatch. |

**Risk:** the Iggy client crate may need a newer Rust than 1.95, the oldest version we support. I told the agent to stop and report if so rather than raise that minimum. It must also check the crate's license and pick the server version from current sources, not memory.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T16:59:13.556302Z
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
- openspec/changes/cr-29-shutdown-cancellation/tasks.md
- openspec/changes/cr-30-pgmq-sql-backend/tasks.md
- openspec/changes/cr-31-hook-emission/tasks.md
- openspec/changes/cr-34-mailbox-composition/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-36-pg-matrix/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
