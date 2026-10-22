## Why

ShutdownPolicy, CancellationSink and SinkOutcome::Requeue exist only as types; the native WorkerPool drops a received task on shutdown (native.rs:603-616).

## What Changes

DrainThenCancel default; stragglers captured to the CancellationSink; Requeue maps to TaskQueue::nack (contract call only, no backend-crate edits); ResourcePool and the native WorkerPool covered; restart redelivery on PG and redb.

## Capabilities

### New Capabilities

- `shutdown-cancellation`: No loss on shutdown

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M2; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-49-durable-dispatch`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
