Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-29-shutdown-cancellation / <task id>`.

## 1. Wire shutdown policy and cancellation capture into both pools

- [x] 1.1 Wire ShutdownPolicy + cancellation channel into ResourcePool and WorkerPool; fix received-task drop
- [x] 1.2 Requeue -> TaskQueue::nack through the contract (no edits inside parking-lot-pg or parking-lot-embedded)
- [x] 1.3 Shutdown and restart tests on PG18 and redb; negative control under --cfg parking_lot_negative_control drops a received task and must be detected
- [x] 1.4 Independent review at the merge gate (no silent loss on any shutdown path)
