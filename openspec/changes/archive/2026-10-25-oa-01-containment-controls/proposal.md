# Proposal: Prepare and enforce post-purge containment controls

## Why

#88 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Implement truthful pre-purge versus post-purge state and prepare a backup/ref-map/reclone/support runbook. Operator executes the approved rewrite; the driver does not force-push. Rotation and KBD signer replacement are explicitly deferred by the 2026-10-24 operator reply and do not block implementation.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- No history rewrite, remote ref deletion, Support submission, credential change or enrollment operation without scoped authorization.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

.github/scripts/supply-chain.sh; .github/pre-purge-roots.txt; docs security runbook. Owner: platform-engineer. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
