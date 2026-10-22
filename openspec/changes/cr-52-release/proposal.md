## Why

The operator requires the library versioned, its crates published and the version tagged. No version of prometheus_parking_lot has ever been published (crates.io checked 2026-10-22); 0.3.0 is the first release and is a breaking change from the 0.2.0 sources.

## What Changes

Versioning ADR (SemVer; workspace.package version 0.3.0 for every crate; inter-crate dependencies pinned to the same minor; MSRV policy). release-plz + git-cliff generate the changelog and release PR. Publishing runs in CI only after the boundary packet passes, using crates.io trusted publishing (OIDC, no long-lived token) configured by the operator per crate, in dependency order: core, runtime-tokio, pg, embedded, iggy, wasm, ffi, facade. Signed annotated tag v0.3.0, GitHub release with notes, docs site deployed for the tag. The long-lived CARGO_REGISTRY_TOKEN in the operator shell is retired.

## Capabilities

### New Capabilities

- `release-management`: Reproducible, approved release

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M5; owner role `platform-engineer` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-39-dependency-hygiene`, `cr-40-benchmarks`, `cr-47-docs-guides`, `cr-53-wasm-parity`, `cr-51-supply-chain-gates`, `cr-36-pg-matrix`, `cr-30-pgmq-sql-backend`, `cr-17-kbd-correction`, `cr-16-docs-truth`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
