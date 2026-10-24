# Proposal: Rehearse a populated PostgreSQL 17→18 upgrade

## Why

#66 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Use image digests and disposable volumes; populate a real 0.3.0 store, perform the 0.4 schema migration in its documented order relative to pg_upgrade, verify invariants and correct the runbook.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Never use operator data directories or non-test Docker volumes; record cleanup handles and backup checks.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

docker images; docs/runbooks/postgres18.md; proposed disposable upgrade rehearsal script and PG suites. Owner: postgres-engineer. Dependencies: oa-06-tenant-idempotency, oa-07-stored-terminal-outcome.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
