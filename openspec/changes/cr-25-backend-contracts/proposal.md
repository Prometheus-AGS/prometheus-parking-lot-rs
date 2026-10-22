## Why

Postgres backends block_on inside the runtime and panic; the queue is at-most-once; no Notifier type exists although CR-32/34 need one. Pulled forward from M2 so backends can fan out in parallel.

## What Changes

Async TaskQueue with lease, ack, nack(requeue|dead-letter), extend; async Mailbox with deliver/consume-once/TTL; Notifier trait; StorageMailbox<S, N> composition type; in-memory reference implementations; a backend conformance test suite every backend must pass.

## Capabilities

### New Capabilities

- `backend-contracts`: Lease semantics; Conformance suite

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M1; owner role `tech-lead` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-18-core-contracts`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
