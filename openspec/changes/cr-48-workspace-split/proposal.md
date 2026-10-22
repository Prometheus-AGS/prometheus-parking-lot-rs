## Why

Backends drag sea-orm/sqlx/iggy/uniffi into every build; the core must build for wasm32 without tokio; four implementers need disjoint ownership. Research (docs/build-performance.md) and the operator decision favour a split; doing it before fan-out avoids every branch rebasing across file moves.

## What Changes

ADR, then one mechanical split: crates/parking-lot-core (dispatcher, semaphore, contracts; no IO, no tokio), crates/parking-lot-runtime-tokio (ResourcePool, native WorkerPool), crates/parking-lot-pg, crates/parking-lot-embedded, crates/parking-lot-iggy, crates/parking-lot-ffi, crates/parking-lot-wasm; the repository root stays the published facade prometheus_parking_lot, re-exporting behind features. Directory names stay short (crates/parking-lot-*) but every published package is named prometheus-parking-lot-* because crates.io treats - and _ alike and parking-lot-core/parking_lot_core belongs to the parking_lot project (checked 2026-10-22). Per-crate include allowlists replace the single exclude list. Constraint checks move from src/ greps to crate-aware checks (cargo tree on core; unsafe grep over crates/** with a documented exemption for generated UniFFI glue in parking-lot-ffi).

## Capabilities

### New Capabilities

- `crate-structure`: Layered workspace

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone M0; owner role `tech-lead` (team `parking-lot-core`, docs/agent-team.md).
- Depends on: `cr-41-toolchain-baseline`.
- Plan of record: `.kbd-orchestrator/phases/phase-correctness-and-durability/plan.md`; source audit `docs/update-plan-10-22-2026.md`.
