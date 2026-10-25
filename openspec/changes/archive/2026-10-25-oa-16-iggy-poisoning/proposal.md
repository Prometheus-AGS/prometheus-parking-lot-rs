# Proposal: Prove no stream publication beyond an abandoned chunk

## Why

#68 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Introduce deterministic fault scheduling at the actual publisher/server boundary without replacing the real sink/reader with mocks. Preserve the documented consumer-dedup requirement; #62 is not solved here.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Use per-run stream/topic names and delete only owned test resources.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

None. This change implements/verifies an existing requirement or changes tooling/runbooks only; skip_specs is explicit.

## Impact

crates/parking-lot-iggy/tests/iggy_stream.rs; stream writer; pinned Iggy test support. Owner: events-engineer. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
