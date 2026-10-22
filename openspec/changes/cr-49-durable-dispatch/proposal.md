## Why

The phase promise is that accepted work survives a process crash, but today ResourcePool enqueues to TaskQueue only when it parks a task, so immediately admitted tasks are never persisted; backends tested in isolation do not prove the pool uses them correctly (plan review CRITICAL).

## What Changes

With a durable backend configured, submit persists before acknowledging acceptance; the dispatcher sources work from TaskQueue::lease; the task is acked only after the mailbox write; long tasks extend their lease with a heartbeat; on restart, unacked work is redelivered by lease expiry. Contract support (any trait additions) via tech-lead handoff; no backend-crate edits.

## Capabilities

### New Capabilities

- `durable-dispatch`: Crash-durable acceptance; Lease extension

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M2; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-21-panic-containment`, `cr-26-pg-lease-queue`, `cr-27-pg-mailbox`, `cr-28-embedded-redb-queue`, `cr-34-mailbox-composition`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
