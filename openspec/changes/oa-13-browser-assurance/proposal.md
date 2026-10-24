# Proposal: Exercise WASM admission and events in a browser

## Why

#65 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Resolve browser/driver and wasm-bindgen runner versions against Cargo.lock and official docs before implementation; preserve no-tokio core graph. Browser installation is an environment prerequisite, not a planning side effect.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Do not change UI design; this is platform verification, not a UI redesign.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

crates/parking-lot-wasm/tests/wasm_pool.rs; wasm_events.rs; .github/workflows/ci.yml. Owner: platform-engineer. Dependencies: oa-10-expiry-events.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
