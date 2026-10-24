## Why

Docs advertise config-driven pools that do not exist; backend parameters (queue_name, path, lease_secs, notifier) have no schema.

## What Changes

Typed config with deny_unknown_fields, backend sections, validation; a builder that constructs pools from config; every JSON example in docs deserializes in a test.

## Capabilities

### New Capabilities

- `configuration`: Config-driven pools

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `tech-lead` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-26-pg-lease-queue`, `cr-28-embedded-redb-queue`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
