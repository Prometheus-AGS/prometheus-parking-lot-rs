## Why

Postgres backends have no running tests; pgmq must be reached through the extension. flint-forge images/postgres18/Dockerfile is the operator-designated basis (PGDG 18, multi-stage, pinned extension sources).

## What Changes

docker/postgres18/Dockerfile: PGDG Postgres 18 base, pgmq built from a pinned source tag, tuned postgresql.conf for queue workloads, init scripts; a pg17 variant for the matrix; compose service; testcontainers helper that builds or reuses the tagged image; CI image build with layer cache.

## Capabilities

### New Capabilities

- `postgres-test-substrate`: Reproducible Postgres substrate

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `postgres-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-48-workspace-split`, `cr-42-ci-rebuild`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
