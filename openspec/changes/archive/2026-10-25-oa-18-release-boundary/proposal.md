# Proposal: Assemble and evaluate one candidate boundary packet

## Why

All accepted phase issues records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Implement this proposed runner and freeze exact per-scenario argv/tool versions before it can run. It coordinates fresh collaborators after all production changes are complete; independent cumulative review has one correction/confirmation cycle. No actual publish or history rewrite inside unattended validation.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Complete phase implementation may be recorded while external publication/containment authorization remains pending; this never certifies a release.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

.github/scripts proposed operational-assurance boundary runner; .github/workflows; phase evidence and release report. Owner: tech-lead. Dependencies: oa-01-containment-controls, oa-02-release-versioning, oa-03-signed-release-tags, oa-04-trusted-publishing, oa-05-template-secret-scan, oa-06-tenant-idempotency, oa-07-stored-terminal-outcome, oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events, oa-11-concurrency-models, oa-12-negative-control-ci, oa-13-browser-assurance, oa-14-postgres-upgrade, oa-15-desktop-assurance, oa-16-iggy-poisoning, oa-17-backend-benchmarks.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
