## Why

Supported environments are PG 17 and 18; queue tables churn and need per-table autovacuum.

## What Changes

CI matrix on the project 17/18 images; per-table autovacuum settings in migrations; docs/runbooks/postgres18.md (checksums, volume path, SCRAM).

## Capabilities

### New Capabilities

- `postgres-compatibility`: Two-version support

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `postgres-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-26-pg-lease-queue`, `cr-42-ci-rebuild`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
