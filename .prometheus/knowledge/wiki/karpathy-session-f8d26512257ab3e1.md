---
type: SessionRecord
id: karpathy-session-f8d26512257ab3e1
title: Karpathy session f8d26512257a
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-24T01:00:17.641015+00:00
created_at: 2026-10-24T01:00:17.641015+00:00
updated_at: 2026-10-24T01:00:17.641015+00:00
revision: 0
---

## Delta

All the review fixes are now in one PR, #76: https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/76. CI is running, so please don't merge it until I confirm the checks pass. GitHub reports it as mergeable.

**What's in #76**
- All 6 critical findings, every warning, and nearly all of the smaller suggestions.
- Every fix was first reproduced with a test that failed on the old code, or the reviewer's claim was disproved with evidence.
- It combines five fix branches.
- #75 (the Postgres fixes), which you'd already merged, is a subset of #76. Bringing it in made no content change.

**Checked on the combined code:** 394 workspace tests, 195 shared queue and mailbox tests, all seven crates packaging and verifying at version 0.3.0, plus lint, docs and the Rust 1.95 build. The tests that need Docker passed on each fix branch individually: Postgres 17 and 18, the crash tests, the shutdown tests and Iggy. The final gate reruns them on the merged code.

**Three judgement calls the agents made:**
1. **Durable pools emit one final event per attempt, not per task.** A task that's retried ends each attempt with "requeued", and only its last attempt ends with a true final event. The docs now say so.
2. **WASM users must disable default features.** The main crate still doesn't build for WASM with its default features, so the docs now tell WASM users to set `default-features = false`, and a CI check keeps that true. Making it build properly would mean changes in about 17 places, so I'd leave it for later.
3. **The stream-replay offset hint is documented, not validated.** The docs' claim about it was narrowed rather than adding validation. A hint set too high still waits instead of reporting an error.

**Moving to the next version (0.4.0):** five items the agents judged too large to change safely this late. I'll file them as issues, like the 20 already open:
- reporting final events from the stored result;
- a CI job that runs every negative control;
- tenant-scoped idempotency keys;
- a task lost if a custom spawner throws away its work;
- a capacity check skipped for queues that already hold tasks before the pool starts.

**Behaviour change for anyone already testing pre-release builds:** durable pools now key results by queue instead of by pool name. Drain any existing durable queues before upgrading. The upgrade notes say this.

**After you merge #76:**
1. A short re-review of only the fixes.
2. The full gate, run with nothing else on the machine.
3. Release 0.3.0.

**Optional cleanup** (agents' leftover build folders and four Iggy test containers that have been running for 5 hours):
`! rm -rf /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/tech-lead/target-nc /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/scheduler-engineer/target/{loom-build,nc-build,loom,nc} && docker rm -f vigilant_bose crazy_hamilton musing_taussig agitated_mcclintock`

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-24T01:00:05.920375Z
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
