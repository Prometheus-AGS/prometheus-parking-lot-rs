## Why

CI is fully red: --all-features cannot resolve on 1.91.1 (pgmq alpha -> sqlx 0.9 needs 1.94), Cargo.lock is gitignored so resolution drifts, benches and examples do not compile, 355 fmt hunks and 157 clippy findings exist. No later change can produce gate evidence until this lands.

## What Changes

Pin rust-toolchain.toml to 1.99.1, set rust-version 1.95 and edition 2024 (cargo fix --edition), commit Cargo.lock, drop the pgmq crate dependency, remove #![deny(warnings)], fix repository/homepage, make benches and examples compile, run one rustfmt pass and resolve clippy findings (fix, or #[expect] with reason).

## Capabilities

### New Capabilities

- `build-baseline`: Reproducible build on the pinned toolchain

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `tech-lead` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-15-containment`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
