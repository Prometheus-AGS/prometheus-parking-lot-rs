Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-21-panic-containment / <task id>`.

## 1. Executor panics never leak capacity or kill the pool

- [x] 1.1 catch_unwind + RAII permits on every execution path; worker respawn; counters
- [x] 1.2 Panic tests for ResourcePool and the native WorkerPool, with a negative control under --cfg parking_lot_negative_control (catch_unwind skipped)
- [ ] 1.3 Independent review at the merge gate (unwind safety, permit release on all paths)
