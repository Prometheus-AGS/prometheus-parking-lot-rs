---
type: SessionRecord
id: karpathy-session-0d76e7da92ad808d
title: Karpathy session 0d76e7da92ad
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-23T16:01:41.737981+00:00
created_at: 2026-10-23T16:01:41.737981+00:00
updated_at: 2026-10-23T16:01:41.737981+00:00
revision: 0
---

## Delta

The docs refresh is up as PR #41, but CI hasn't finished on it, so don't merge it yet. #40 was merged before 8 of its 17 CI checks had finished, so CI on `main` at `34a53b5` is still running. I'll report both results when they're in.

**PR #41 (docs refresh):** https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/pull/41
- **What changed:** the website concept pages and the design docs had been describing the crate as it was before rounds 6–11. Every claim is now rewritten from the code on `main`.
- **Independent check:** a separate reviewer compared the rewrite with the code. It found no made-up names, but it found nine overstatements, and all were fixed. Examples:
  - A durable pool is a separate type, not just a `ResourcePool` pointed at a database.
  - Durable queues only approximately keep priority order.
- **My additions on top:**
  - #40 made one sentence false (it said pgmq couldn't be set from config), so I corrected it.
  - I fixed two CHANGELOG errors. The removal of sea-orm was listed under "Added", and an older entry still described the Postgres queues as future work.
- **Local results:** the site builds with broken links set to fail the build. The tests that check the JSON config examples in the docs pass, and so do the doc tests.

**Two items need a look from the owning roles:**
- The agent reworded a hard rule in `docs/agent-context.md`. It was "never import sea-orm" and now says "parking-lot-core has no runtime, IO, database or network dependencies". It's accurate, but it changes a rule, so tech-lead should confirm it.
- It also edited `ARCHITECTURE.md` and `build-performance.md`, which other roles own.

**Still running:** cr-31 (lifecycle events). When it lands, I'll merge `main` into it, since it also touches `lib.rs` and the config builder that #40 changed, and update the events page on the site.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: unknown
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-23T15:59:24.291378Z
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
- openspec/changes/cr-34-mailbox-composition/tasks.md
- openspec/changes/cr-35-test-targets/tasks.md
- openspec/changes/cr-36-pg-matrix/tasks.md
- openspec/changes/cr-37-config-schema/tasks.md
- openspec/changes/cr-38-wasm-target/tasks.md
- openspec/changes/cr-44-mobile-desktop-bindings/tasks.md
- openspec/changes/cr-49-durable-dispatch/tasks.md
- .kbd-orchestrator/.position.json.58e66893-07a6-41a4-bae1-d0dd095eb786.tmp
