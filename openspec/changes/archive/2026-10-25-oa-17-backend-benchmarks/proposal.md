# Proposal: Measure PG throughput, contention and memory

## Why

#71 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Freeze hardware/OS/CPU/thread counts, toolchain, features and image digests; no speedup claim if confidence intervals overlap noise. This issue requires reproducible measurement, not an invented numerical performance target. No release benchmark during implementation.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- One build/benchmark writer, no active agents/builds on the benchmark host; no privileged host tuning without approval.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

benches/; benches/README.md; docs/ARCHITECTURE.md section 7; disposable PG fixtures. Owner: scheduler-engineer. Dependencies: oa-06-tenant-idempotency, oa-07-stored-terminal-outcome, oa-08-discarded-spawn, oa-09-backend-admission, oa-10-expiry-events.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
