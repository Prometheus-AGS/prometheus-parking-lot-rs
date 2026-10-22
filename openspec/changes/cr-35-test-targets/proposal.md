## Why

tests/unit and tests/integration are never compiled and use the removed testcontainers clients::Cli/RunnableImage API; the coverage baseline overstates.

## What Changes

Register the suites as test targets in their crates, port to the current testcontainers API using the CR-43 image, quarantine (with issue links) any test that encodes the old broken behaviour rather than deleting it.

## Capabilities

### New Capabilities

- `test-infrastructure`: All suites run

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-48-workspace-split`, `cr-43-postgres18-image`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
