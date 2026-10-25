# Proposal: Reject impossible pre-populated adapter work

## Why

#82 records the observed missing behavior or assurance. See the phase assessment and issue snapshot; no new production failure was reproduced during planning.

## What Changes

- Use the existing Dispatcher::check_capacity contract on first observation; persist or emit a terminal reason appropriate to the adapter, without silently dropping entries or looping forever.
- Include production regression fixtures and the documented boundary oracle; production execution is deferred until the complete implementation set.
- Keep one dispatcher owner; distinguish impossible total/kind cost from temporary lack of free capacity.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `capacity-admission`: Reject impossible pre-populated adapter work; behavioral delta below.

## Impact

TaskQueueAdapter and dispatcher first-observation path; core/runtime admission tests. Owner: scheduler-engineer. Dependencies: none.
Phase plan: ../../../.kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md. No production code, credentials, account setting, history rewrite or publication is changed by this proposal.
