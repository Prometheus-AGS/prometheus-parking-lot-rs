## Why

Operator decision: drop the alpha pgmq client crate that broke MSRV; reach the pgmq extension (>= 1.8, PG18) through SQL.

## What Changes

Implement the TaskQueue contract with pgmq.send/read/delete/archive and visibility timeouts as leases; one pgmq queue per priority level; passes the conformance suite on the PG18 image.

## Capabilities

### New Capabilities

- `durable-queue-pgmq`: pgmq-backed queue

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M2; owner role `postgres-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-26-pg-lease-queue`, `cr-43-postgres18-image`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
