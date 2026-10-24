## Why

Notifier failure must never lose a stored result; there is no composition today.

## What Changes

StorageMailbox<S, N> stores first, notifies off-path with bounded retries; notifier failure is logged and surfaced, never fatal to storage.

## Capabilities

### New Capabilities

- `result-delivery`: Storage before notification

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M3; owner role `events-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-25-backend-contracts`, `cr-27-pg-mailbox`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
