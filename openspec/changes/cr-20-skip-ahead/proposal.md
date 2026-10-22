## Why

Strict head-of-line dispatch wastes capacity when a large task waits behind free units (PARKING_LOT.md section 3.3).

## What Changes

Dispatch a smaller fitting task when the head does not fit, bounded by a configurable starvation guard that reserves capacity for the head after N skips.

## Capabilities

### New Capabilities

- `capacity-admission`: Bounded skip-ahead

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M1; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-19-dispatcher-semaphore`, `cr-31-hook-emission`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
