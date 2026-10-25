# Proposal: Emit terminal events for explicitly pruned work

## Why

#57 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Return pruned task metadata through the contract; implement across queue adapters, retain the public count if compatible, and update conformance and changelog.
- **BREAKING:** changes the public contract across all implementers and requires migration/changelog evidence.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Do not emit terminal events while holding dispatcher locks; preserve existing event causality and panic boundaries.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `lifecycle-events`: Emit terminal events for explicitly pruned work; behavioral delta below.

## Impact

crates/parking-lot-core/src/contracts/queue.rs; dispatcher and all TaskQueue implementers; ResourcePool pruning; lifecycle tests. Owner: tech-lead. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
