## Why

Benchmarks do not compile; ARCHITECTURE.md numbers are unverifiable.

## What Changes

Criterion suites for dispatch throughput, latency and utilization; CI job storing results; ARCHITECTURE.md section 7 rewritten from them (tech-lead).

## Capabilities

### New Capabilities

- `performance-evidence`: Reproducible numbers

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M4; owner role `scheduler-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-21-panic-containment`, `cr-22-result-slots`, `cr-20-skip-ahead`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
