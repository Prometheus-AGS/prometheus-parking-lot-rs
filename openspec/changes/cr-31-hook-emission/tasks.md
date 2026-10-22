Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-31-hook-emission / <task id>`.

## 1. Emit lifecycle events at every transition

- [ ] 1.1 Emit events from the dispatcher and pools; builder registration (tech-lead handoff); hooks.rs API adjustments via events-engineer handoff
- [ ] 1.2 Ordering tests and overhead benchmark
- [ ] 1.3 Independent review at the merge gate (event completeness, overhead)
