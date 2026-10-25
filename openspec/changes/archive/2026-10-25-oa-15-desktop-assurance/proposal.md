# Proposal: Build and launch the Tauri embedded example

## Why

#67 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Keep the incumbent UI unchanged. Exact supported desktop launch matrix is frozen in the contract: builds on all three OS, actual GUI restart operation on macOS; if Linux/Windows runtime support is claimed, require corresponding launch evidence before that claim.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- No unattended GUI input to unrelated apps; no production user directories; no new paid runner or host package installation without approval.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

bindings/tauri-desktop; .github/workflows/platform.yml; desktop launch/restart evidence runbook. Owner: platform-engineer. Dependencies: oa-06-tenant-idempotency.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
