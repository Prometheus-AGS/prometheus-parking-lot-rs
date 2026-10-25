# Proposal: Scope result idempotency to tenant/mailbox key

## Why

#80 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Tech-lead owns the public contract and conformance change; postgres-engineer and events-engineer implement the two backends after that contract. Preserve old database/store fixtures and require explicit safe migration.
- **BREAKING:** changes the public contract across all implementers and requires migration/changelog evidence.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- No production store mutation; migration fixtures are isolated copies with manifests and rollback/read-compatibility policy.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `backend-contracts`: Scope result idempotency to tenant/mailbox key; behavioral delta below.

## Impact

crates/parking-lot-core/src/contracts (ResultMailbox and conformance); crates/parking-lot-pg/src/mailbox and migrations; crates/parking-lot-embedded/src/mailbox.rs; docs/API.md. Owner: tech-lead. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
