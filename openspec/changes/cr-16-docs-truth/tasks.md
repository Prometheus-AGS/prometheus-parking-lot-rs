Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-16-docs-truth / <task id>`.

## 1. Archive invalid docs and make README examples compile

- [x] 1.1 Archive invalid docs (PLAN-/SPEC-/TASKS-*, primitives docs, stale reports) to docs/archive/ with an index of reasons
- [x] 1.2 Rewrite README.md against the real API and wire it as a doctest on the facade (tech-lead applies the lib.rs attribute)
- [ ] 1.3 Independent review at the merge gate (no stale API claims remain)
