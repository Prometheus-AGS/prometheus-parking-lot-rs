## Why

~30% of docs/ describes a different crate (Mutex/RwLock/Once/Condvar specs); README samples use non-existent APIs; the old CLAUDE.md content lives on in docs.

## What Changes

Move invalid docs to docs/archive/ with a one-line reason each, reset TODO/API, and make every README code block a compiled doctest (doc = include_str! on the facade).

## Capabilities

### New Capabilities

- `documentation-integrity`: Docs describe this crate

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `product-manager` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-48-workspace-split`, `cr-18-core-contracts`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
