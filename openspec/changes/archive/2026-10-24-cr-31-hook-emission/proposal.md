## Why

HookBus and emit_* are exported but never called by any pool. Emission points live in the scheduler-owned dispatcher and pools, so the scheduler owner writes them after every transition (including cancellation and requeue) exists.

## What Changes

Emit submitted/parked/started/completed/failed/cancelled/requeued/rejected from the dispatcher and pools; hooks registered on the builder (tech-lead handoff); hook API changes in hooks.rs via events-engineer handoff; zero overhead when no hooks are registered.

## Capabilities

### New Capabilities

- `lifecycle-events`: Ordered lifecycle events

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M3; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-29-shutdown-cancellation`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
