## Why

The mailbox block_ons inside the runtime and cannot be read back.

## What Changes

consume as DELETE ... RETURNING, TTL eviction, ON CONFLICT on the idempotency key, async sqlx; passes the mailbox conformance suite.

## Capabilities

### New Capabilities

- `durable-mailbox-postgres`: Consume-once delivery

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M2; owner role `postgres-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-26-pg-lease-queue`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
