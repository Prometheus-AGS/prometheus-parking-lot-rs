# Proposal: Gate trusted publishing on candidate evidence and approval

## Why

#85 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Split preparation, evidence validation and approval-gated publication; require candidate SHA, contract digest and tag identity to agree. Owner GQAdonis/operator coordinates repository and crates.io permissions; availability is unverified until an attributable account receipt exists.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- No credentials, account settings or third-party resources changed during planning. Publication is not authorized by plan adoption.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `release-management`: Gate trusted publishing on candidate evidence and approval; behavioral delta below.

## Impact

.github/workflows planned release workflow; docs/PUBLISHING.md; seven crates.io publisher settings (operator). Owner: platform-engineer. Dependencies: oa-01-containment-controls, oa-02-release-versioning, oa-03-signed-release-tags.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
