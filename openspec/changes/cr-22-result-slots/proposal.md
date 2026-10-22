## Why

retrieve_async parks a spawn_blocking thread per waiter; a few hundred waiters exhaust the blocking pool; unretrieved results live forever.

## What Changes

Per-slot oneshot/Notify for async waiters, blocking_recv for the sync API, u64 keys, TTL sweeper defaulting to the task timeout.

## Capabilities

### New Capabilities

- `result-retrieval`: Non-blocking retrieval

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M1; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-19-dispatcher-semaphore`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
