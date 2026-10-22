## Why

Mobile and desktop reach is a phase goal; no bindings or examples exist.

## What Changes

parking-lot-ffi with UniFFI async exports over the embedded backend; cargo-ndk Android and iOS (device + sim) check jobs; a Tauri example using the embedded queue.

## Capabilities

### New Capabilities

- `platform-targets`: Mobile and desktop reach

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-25-backend-contracts`, `cr-37-config-schema`, `cr-28-embedded-redb-queue`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
