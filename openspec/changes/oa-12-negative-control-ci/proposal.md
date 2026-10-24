# Proposal: Run every scenario negative control in CI

## Why

#79 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Create a checked manifest from actual cfg sites and expected assertion/test identities. Existing controls include S-1–S-9 where defined, S-11,S-16–S-21,S-33; use enumerated source inventory rather than assuming every integer exists. Independent targets avoid RUSTFLAGS cache collisions.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- No CI green result from arbitrary nonzero exit; retain diagnostic classifier and all raw output.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

.github/workflows/ci.yml; proposed negative-control manifest/runner; existing scenario suites. Owner: platform-engineer. Dependencies: oa-06-tenant-idempotency, oa-07-stored-terminal-outcome, oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events, oa-11-concurrency-models.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
