# Proposal: Emit durable terminal events from the stored result

## Why

#78 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Extend DeliveryOutcome duplicate metadata to retain authoritative terminal status in idempotency records, including consumed-result cases. Update all ResultMailbox implementers/conformance and migrate retained metadata; do not fetch destructive consume just to learn status.
- **BREAKING:** changes the public contract across all implementers and requires migration/changelog evidence.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Contract/migration changes depend on oa-06; no acknowledged task may lose its durable outcome.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `backend-contracts`: Emit durable terminal events from the stored result; behavioral delta below.

## Impact

crates/parking-lot-core/src/contracts; PG/redb mailbox implementations; crates/parking-lot-runtime-tokio/src/durable_pool/mod.rs. Owner: tech-lead. Dependencies: oa-06-tenant-idempotency.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
