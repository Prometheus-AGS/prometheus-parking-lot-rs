## Why

Unused dependencies slow builds; a published crate needs semver discipline.

## What Changes

cargo udeps clean, remove leftovers (sea-orm if unused, yaque), cargo-semver-checks in CI.

## Capabilities

### New Capabilities

- `continuous-integration`: Lean, semver-checked dependency graph

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-26-pg-lease-queue`, `cr-28-embedded-redb-queue`, `cr-32-iggy-events`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
