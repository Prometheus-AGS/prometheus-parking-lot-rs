## Why

Implementers must build against merged contracts; ResourcePool threads many Arcs through static futures; sync_wake_worker_loop is public and discards work; executors cannot report failure; time is untestable without a Clock.

## What Changes

Introduce Arc<Inner> for ResourcePool; delete sync_wake_worker_loop; Clock trait (monotonic + wall) with a test clock; TaskExecutor returns Result<T, TaskError>; extend the #[non_exhaustive] error taxonomy (ExecutorPanicked, Timeout, Cancelled, CapacityExceeded); monotonic task sequence for FIFO (folds in CR-24); the cost type is defined now as a resource vector (a scalar cost is the one-dimension case) so backends, config and FFI never build on a scalar that cr-23 would break.

## Capabilities

### New Capabilities

- `core-contracts`: Executors report failure; Deterministic ordering

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M1; owner role `tech-lead` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-48-workspace-split`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
