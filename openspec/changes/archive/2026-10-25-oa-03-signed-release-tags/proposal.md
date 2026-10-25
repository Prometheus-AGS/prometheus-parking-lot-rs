# Proposal: Verify operator-signed release tags

## Why

#87 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Do not generate, import or copy private signing keys. Operator selects existing release authority; workflow verifies an explicit identity and exact commit, not merely presence of a signature.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Release signing setup is a separate scoped account/key action; no private key bytes in repo, workflow output or receipts.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `release-management`: Verify operator-signed release tags; behavioral delta below.

## Impact

.github/workflows planned release workflow; .github/scripts signing verifier; docs/PUBLISHING.md. Owner: platform-engineer. Dependencies: oa-02-release-versioning.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
