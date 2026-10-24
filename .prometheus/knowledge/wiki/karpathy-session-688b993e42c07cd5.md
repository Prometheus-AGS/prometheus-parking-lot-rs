---
type: SessionRecord
id: karpathy-session-688b993e42c07cd5
title: Karpathy session 688b993e42c0
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T19:51:42.726815+00:00
created_at: 2026-10-23T19:51:42.726815+00:00
updated_at: 2026-10-23T19:51:42.726815+00:00
revision: 0
---

## Delta

cr-23, multi-resource admission, is up as PR #48: https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/48. CI is still running, so please hold the merge until I confirm it. It's the last change in round 14. `main` is green after #47, including the Postgres and coverage jobs that were still running when #47 merged.

**What it does:** a pool can now set a limit per resource, for example `"resource_limits": { "GpuVram": 16 }`. A task starts only when it fits within every limit at the same moment, and all of its resources are reserved in one step. Pools with no per-resource limits work exactly as before, and existing config files load unchanged. Config validation also rejects limits that look like a units mistake, such as VRAM given in MB.

**Verified locally:**
- A concurrency model checker explored every thread interleaving and found no case where any resource goes over its limit.
- The negative control, which checks only the first resource, makes both models fail, as intended.
- The memory checker (Miri) found nothing in the admission code.
- Randomised tests ran at 50,000 cases.
- Earlier suites still pass: no starvation, event ordering, shutdown, config, and the WASM tests.
- The full workspace suite passes: 349 tests.

**Performance trade-off:**
- Pools that use one resource now admit tasks 2–4% slower in the dispatcher micro-benchmark.
- At the pool level, submit timings show no measurable difference.
- The agent got the cost down from 8–11%. What's left is real and reproducible.
- I think that's acceptable for per-resource admission. If you'd rather not ship a regression, say so and I'll send it back for another pass.

**Breaking changes:** a few internal admission types changed, and code that builds config structs field by field needs the new field. All of it is listed in the CHANGELOG.

**Known limits:**
- When a task is rejected for exceeding capacity, the error doesn't say which resource was too small.
- Per-resource usage is exposed through stats, not through lifecycle events.

**My fix on top:** a doc comment on `ResourceCost` still said per-resource admission was future work. I updated it.

**For you:** the agent left three build folders in its worktree, about 600 MB in total. Its attempt to delete them was denied, so I didn't remove them either:
`! rm -rf /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/scheduler-engineer/{target-loom,target-miri,target-nc19}`

**Next:** after #48 merges, round 15 starts: cr-40 (benchmarks), cr-47 (guides) and cr-53 (WASM parity). The end-of-phase gate and the release follow.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T19:51:39.165312Z
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
- openspec/changes/cr-20-skip-ahead/tasks.md
- openspec/changes/cr-21-panic-containment/tasks.md
- openspec/changes/cr-22-result-slots/tasks.md
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
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
