# Proposal: Settle accepted work when a spawner discards its future

## Why

#81 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Establish ownership/settlement before handing the future to Spawn. Preserve panic containment and avoid duplicate terminal reporting. Do not claim durable execution for an in-memory ResourcePool.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- No new dependency from core to an async runtime; preserve capacity and shutdown ordering.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `fault-containment`: Settle accepted work when a spawner discards its future; behavioral delta below.

## Impact

crates/parking-lot-runtime-tokio/src/resource_pool.rs; shutdown ledger; runtime integration tests. Owner: scheduler-engineer. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
