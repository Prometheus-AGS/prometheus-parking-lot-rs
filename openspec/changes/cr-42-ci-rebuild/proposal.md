## Why

Every CI job fails on main; caching and locking are absent; cargo-deny fails on RUSTSEC-2025-0111 (tokio-tar via testcontainers) and licenses.

## What Changes

Swatinem/rust-cache@v2 (save-if main), --locked everywhere, nextest, jobs for fmt/clippy/test(1.99.1, 1.95)/docs/deny/coverage; upgrade testcontainers to clear the advisory and fix deny.toml licenses; per-role aliases updated for the workspace.

## Capabilities

### New Capabilities

- `continuous-integration`: Required CI jobs pass on main

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-48-workspace-split`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
