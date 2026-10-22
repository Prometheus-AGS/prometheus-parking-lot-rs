## Why

WorkerPool (native and WASM) never enforces max_units (C1); ResourcePool has a lost wakeup (C3), pop-then-push task loss (C4) and a max_queue_depth TOCTOU; WorkerPool executions are unbounded.

## What Changes

One dispatcher (parking-lot-core, no tokio) owns the priority queue and a unit-weighted semaphore; submit performs depth check + enqueue atomically; dispatch peeks and pops only when the head fits. ResourcePool and the native WorkerPool (parking-lot-runtime-tokio) use it, and native WorkerPool executions get the same timeout/deadline budget as ResourcePool. The WASM pool is ported by platform-engineer in cr-38. Loom models run under `--cfg parking_lot_loom` on the core crate only; negative controls use the test-only `--cfg parking_lot_negative_control`, never a Cargo feature.

## Capabilities

### New Capabilities

- `capacity-admission`: Capacity invariant; No lost wakeup; Bounded execution

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M1; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-18-core-contracts`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
