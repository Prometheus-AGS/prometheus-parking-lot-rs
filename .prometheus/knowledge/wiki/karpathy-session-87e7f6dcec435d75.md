---
type: SessionRecord
id: karpathy-session-87e7f6dcec435d75
title: Karpathy session 87e7f6dcec43
tags:
- karpathy
- session-learning
sources:
- resource: session:d281c3f1-466c-41cb-b006-ce00f1aa43b6
generated:
  by: pk/1.11.0
  at: 2026-10-22T11:59:11.077765+00:00
created_at: 2026-10-22T11:59:11.077765+00:00
updated_at: 2026-10-22T11:59:11.077765+00:00
revision: 0
---

## Delta

Completed adversarial-review — PASS (0/7/2)
Completed kbd-assess — phase-correctness-and-durability (step 0 of 0)

The assessment is written. On current `main` (`90a6194`), none of the phase's six milestone goals is met. The seventh, delivery by the agent team, is partial: the team is installed but no role has run yet. The assess stage is recorded in KBD at revision 20, and the handoff for planning is written.

**Two problems are worse than the audit described:**
- **The Postgres backends don't work at all.** Every database call blocks inside the async runtime, which panics, so any Postgres-backed pool that has to queue a task crashes. The code comments say "SKIP LOCKED" but the query doesn't use it.
- **Capacity limits are broken in both `WorkerPool` versions**, native and WASM. Neither has a timeout, because the #2 fix only covered `ResourcePool`.

**Other verified gaps:**
- the native worker silently drops a task it has already taken if shutdown happens at that moment;
- executor panics are never caught;
- the public `sync_wake_worker_loop` throws work away;
- hooks are never emitted;
- no notifier type exists, though two later CRs depend on one.

**Builds:**
- **Compiles but nothing else does:** the library and tests compile with default features. The bench and both examples don't, so the `ck` and `t` shortcuts from #5 can't pass yet.
- **CI on `main`:** every job fails, for the reasons found earlier.
- **Test suites:** `tests/unit` and `tests/integration` are never built, so the Postgres code has no running tests.

**Review:** the adversarial review passed, with 7 warnings and 2 suggestions, all now applied to the assessment. It ran as a same-model-family subagent, not cross-model, for two reasons:
- **No judge models configured:** no providers are set up for liter-llm.
- **Packet builder blocked:** the Trail of Bits modern-python plugin's `python3` shim refuses to run the skill pack's packet builder. That conflict will hit every adversarial review until it's fixed.

**Decisions needed from you before planning:**
1. **Edit `constraints.md` for `.codex/`.** It currently says `.codex/` is tracked tooling that must not be deleted, which contradicts CR-15's purge. It also claims `.codex/` is excluded from the package, which is false.
2. **Pull the pgmq decision forward.** The blocking constraints use `--all-features`, which can't build on Rust 1.91.1 while `pgmq` is present. The plan defers the pgmq decision to CR-30 in M2. I recommend dropping or pinning pgmq in a new M0 build-baseline change, alongside fixing all build targets, committing `Cargo.lock` and the format pass.
3. **Risk acceptance.** May the breaking API changes in M1/M2 ship as 0.3.0? Is it acceptable for CI to stay red until M0 is done?

Answer those and I'll run `/kbd-plan`. The assessment, review findings and handoff are uncommitted on `main`'s working tree; I'll put them on a branch with the plan.

**KBD status:** position `phase-correctness-and-durability › assess` (complete), revision 20, lifecycle Ready. Next: `/kbd-plan`. The `.codex/auth.json` tokens still need revoking.

## Root Cause

No explicit root-cause section was captured; preserve this as a session record, not an inferred diagnosis.

## Corrective Actions

Review and promote only reusable findings.

## Session Metadata

- Harness: claude-code
- Session: d281c3f1-466c-41cb-b006-ce00f1aa43b6
- Captured: 2026-10-22T11:31:47.978373Z
- Project: /Volumes/T7/Projects/prometheus/prometheus-parking-lot

## Changed Paths

- .kbd-orchestrator/current-waypoint.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/hooks-status.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/hooks.log.jsonl
- .kbd-orchestrator/phases/phase-correctness-and-durability/progress.json
- .kbd-orchestrator/position-reminder.txt
- .kbd-orchestrator/position.json
- .kbd-orchestrator/model-preflight.json
- .kbd-orchestrator/phases/phase-correctness-and-durability/assessment.md
- .kbd-orchestrator/phases/phase-correctness-and-durability/handoffs/
- .kbd-orchestrator/phases/phase-correctness-and-durability/prior-context.md
- .kbd-orchestrator/phases/phase-correctness-and-durability/review/
- .kbd-orchestrator/phases/phase-correctness-and-durability/sycophancy/
- .serena/
