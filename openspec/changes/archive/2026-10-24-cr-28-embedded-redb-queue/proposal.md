## Why

The JSONL queue is not crash-safe and ignores priority; desktop/Tauri needs durable local parking.

## What Changes

redb-backed lease queue and mailbox (one-day rustqueue spike recorded first), priority honoured, fault-injection test at every crash point; Yaque code removed.

## Capabilities

### New Capabilities

- `embedded-queue`: Crash safety

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M2; owner role `events-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-25-backend-contracts`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
