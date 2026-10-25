# Proposal: Make template scans precise without hiding real secrets

## Why

#89 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Prefer removing ambiguous placeholder material or narrowly fingerprinted example exceptions with rationale. Do not blanket-allowlist template folders or weaken default rules.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Synthetic nonfunctional test values only; never insert a real key to test a detector.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

.gitleaks.toml; docs/coding-standards/README.md; .e2b-template/SETUP.md. Owner: platform-engineer. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
