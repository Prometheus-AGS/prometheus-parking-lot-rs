Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-18-core-contracts / <task id>`.

## 1. Core contracts: Arc<Inner>, Clock, Result executors, error taxonomy

- [x] 1.1 Arc<Inner> refactor of ResourcePool; delete sync_wake_worker_loop; Clock trait + TestClock
- [x] 1.2 Result-returning TaskExecutor contract and error taxonomy; monotonic FIFO sequence with consistent Eq/Ord; resource-vector cost type (serde, one-dimension default)
- [x] 1.3 Migrate existing tests and examples to the new contracts; CHANGELOG 0.3.0 entry
- [ ] 1.4 Independent review at the merge gate (API soundness, Send/Sync bounds, semver notes)
