## Why

cr-20/23/31 change the shared dispatcher after cr-38 ports the WASM pool; without a follow-up the WASM pool silently misses vector admission and lifecycle events (plan review round 2).

## What Changes

Bring the WASM pool to parity: vector admission, skip-ahead, lifecycle events. Durable dispatch does not apply on the web target (no durable backend in the browser); this is documented.

## Capabilities

### New Capabilities

- `platform-targets`: WASM parity

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-38-wasm-target`, `cr-23-multi-resource`, `cr-31-hook-emission`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
