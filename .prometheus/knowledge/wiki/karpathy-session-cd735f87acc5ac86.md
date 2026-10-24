---
type: SessionRecord
id: karpathy-session-cd735f87acc5ac86
title: Karpathy session cd735f87acc5
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T18:27:09.846162+00:00
created_at: 2026-10-23T18:27:09.846162+00:00
updated_at: 2026-10-23T18:27:09.846162+00:00
revision: 0
---

## Delta

Both round-13 PRs are open. #43 is ready to merge now; #44 is still running CI. No Rust upgrade turned out to be needed.

**Rust version:** the Iggy client (0.11.0) declares 1.95 as its minimum, and it builds on 1.95. So the minimum stays at 1.95, and the toolchain stays at 1.99.1, the current latest release. I recorded that outcome next to your instruction in `.prometheus/decisions.md`, which stands for any future dependency that needs more.

**Ready to merge:**
- **#43 (cr-20, skip-ahead):** passed all 18 CI checks. https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/43
- Skip-ahead is still on by default. Say so if you'd rather it be opt-in.

**CI running:**
- **#44 (cr-32, Iggy events):** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/44

What #44 adds:
- **What gets sent:** a new `parking-lot-iggy` crate, behind an `iggy` feature, sends lifecycle events and "result ready" notices to an Iggy server.
- **Never slows scheduling:** sending happens on a background worker with a bounded queue. When the server falls behind, events are dropped and counted. The worker retries failed sends a limited number of times.
- **Tested against a paused server:** with 2,000 events, the hook stayed under 50 ms, scheduling dropped nothing, and sending recovered once the server resumed.
- **Negative control:** random message IDs plus sending in-line on the scheduling path made both the ID test and the latency test fail, as they should.

Before you rely on #44:
- **Duplicates are possible:** the current Iggy server (0.9) no longer removes duplicate message IDs itself. The publisher skips repeats it has sent recently, but a restarted publisher can store a second copy with the same ID. Consumers need to de-duplicate on the ID. This is documented and tested.
- **Docker settings:** the Iggy server needed two special settings to start under Docker on this Mac. The new Iggy CI job will be the first test on a Linux runner.
- **Dependency size:** the Iggy client adds 99 crates, but they only compile when the `iggy` feature is on.
- **Hook not attached automatically:** the config builder doesn't attach the Iggy hook. Applications register it themselves.

**Merge order:** #43, then #44. Both touch the config code, so #44 may need a small rebase after #43 goes in. I'll do it if GitHub reports a conflict.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T18:27:05.007602Z
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
