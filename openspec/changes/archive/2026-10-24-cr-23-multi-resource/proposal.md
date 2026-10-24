## Why

A single unit count cannot express a task needing 8 GB VRAM and one worker slot (PARKING_LOT.md section 5).

## What Changes

The resource-vector cost type already exists (cr-18); this change makes the semaphore and dispatcher admit by every dimension, adds multi-dimension pool limits to config, and emits capacity events per dimension.

## Capabilities

### New Capabilities

- `capacity-admission`: Multi-dimensional admission

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M1; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-20-skip-ahead`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
