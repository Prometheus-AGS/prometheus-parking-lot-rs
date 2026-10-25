# Proposal: Choose and automate seven-crate versioning

## Why

#86 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Recommend lockstep to match the existing uniform 0.3.0 release. Validate official release-plz/git-cliff capabilities and versions before introducing dependencies; freeze exact tool versions and executable dry-run argv in the boundary manifest during implementation.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- No registry publishing in a dry run. New tooling/version choices verified from primary sources before installation.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `release-management`: Choose and automate seven-crate versioning; behavioral delta below.

## Impact

Cargo.toml; crates/*/Cargo.toml; release-plz/git-cliff configuration; docs/adr; CHANGELOG.md. Owner: tech-lead. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
