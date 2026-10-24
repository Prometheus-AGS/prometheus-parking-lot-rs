Model and worker assignments for these tasks: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md` → Task model assignments, rows keyed `phase-correctness-and-durability / cr-19-dispatcher-semaphore / <task id>`.

## 1. Unit-weighted semaphore and priority dispatcher shared by every pool

- [x] 1.1 Implement the dispatcher + unit-weighted semaphore; atomic depth-check/enqueue; peek-then-pop
- [x] 1.2 Port ResourcePool and the native WorkerPool onto the dispatcher; add execution budgets to the native WorkerPool
- [x] 1.3 loom models (loom_capacity, loom_wakeup) on parking-lot-core under --cfg parking_lot_loom with negative controls under --cfg parking_lot_negative_control; proptest suites for the tokio pools; budget_ tests (wedged executor, cancel witness) for ResourcePool and the native WorkerPool; miri on atomics paths
- [x] 1.4 Independent review at the merge gate (memory ordering, lost-wakeup proof, negative control)
