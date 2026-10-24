# Proposal: Model ledger races and kind-limited release

## Why

#64 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Model the production state machine with explicit mapping to source ordering; no runtime dependency in core normal graph. Bound exploration and retain seed/preemption/compiler receipts.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- A model is supporting evidence; real runtime/backends and shutdown/crash paths remain required.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

crates/parking-lot-core/tests/loom_*; runtime shutdown ledger model and cfg wiring; .github/workflows/ci.yml. Owner: scheduler-engineer. Dependencies: oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
