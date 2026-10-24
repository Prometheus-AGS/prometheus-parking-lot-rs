## Why

No catch_unwind exists on any execution path (C2): a panic leaks units, strands result slots and kills WorkerPool workers.

## What Changes

Native pools only (ResourcePool, native WorkerPool): catch_unwind around executor futures, RAII semaphore permits so release happens on every exit path, worker respawn, failed_tasks accounting, panic delivered as a typed failure. On wasm32-unknown-unknown panics abort the module on stable Rust; cr-38 documents that limit.

## Capabilities

### New Capabilities

- `fault-containment`: Panic containment

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M1; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-19-dispatcher-semaphore`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
