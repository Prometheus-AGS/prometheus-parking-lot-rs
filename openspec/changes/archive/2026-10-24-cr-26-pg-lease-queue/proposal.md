## Why

The current queue panics inside the runtime (block_on), deletes on dequeue (at-most-once) and claims SKIP LOCKED while using plain FOR UPDATE.

## What Changes

Single-statement lease with FOR UPDATE SKIP LOCKED (plan section 7.4), ack/nack/extend, LISTEN/NOTIFY wake-up, lease reaper, not_before backoff, migrations; raw sqlx 0.9.0 (stable 2026-05-21, rust-version 1.94 <= MSRV 1.95; re-check at implementation and record the date); passes the conformance suite. sea-orm stays until cr-27 ports the mailbox.

## Capabilities

### New Capabilities

- `durable-queue-postgres`: At-least-once with leases

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M2; owner role `postgres-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-25-backend-contracts`, `cr-35-test-targets`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
