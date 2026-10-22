# Agent context — prometheus_parking_lot

Project facts for agents. Read before changing code. `AGENTS.md` points here;
it stays small because the managed operating rules already fill its word budget.

## What it does

Tasks carry a `ResourceCost`. A `ResourcePool` admits a task when
`active_units + cost <= max_units`; otherwise it parks the task in a priority
queue and wakes the next task that fits when capacity frees. Results go to a
`Mailbox` so disconnected clients can retrieve them later. Shutdown, timeouts,
cancellation capture and idempotency follow the decisions recorded in
`.kbd-orchestrator/phases/phase-certification-remediation/decision-*.md`.

## Layout

```text
src/core/      pure domain + scheduler (ResourcePool, worker_pool/) — no infra imports
src/config/    serde config models
src/builders/  config -> core abstractions
src/infra/     queue/ (in-memory, Postgres, pgmq), mailbox/ (memory, yaque, Postgres), entities/ (SeaORM)
src/runtime/   runtime adapters (tokio)
src/util/      Clock and shared utilities
migrations/    Postgres schema (pl_queue_jobs, pl_mailbox_messages, pgmq)
tests/         unit/, integration/ (testcontainers Postgres — needs Docker), contract/, root suites
benches/       criterion (queue_bench)
```

Features: `tokio-runtime` (default), `postgres` (sea-orm, sqlx, chrono),
`pgmq-ext` (postgres + pgmq). Design references: `docs/ARCHITECTURE.md`,
`docs/API.md`, `README.md`, `docs/PUBLISHING.md`.

## Commands (mirror CI in `.github/workflows/`)

```bash
cargo check  --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo test   --all-features                       # Docker required for Postgres tests
cargo fmt    --all -- --check
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features
cargo deny check
```

## Hard rules

- `unsafe_code = "deny"` and `missing_docs = "deny"` (Cargo.toml lints). Clippy
  `all`, `pedantic`, `nursery` are warn and CI runs `-D warnings`.
- `src/core` never imports `sea_orm`, `sqlx`, `pgmq` or `reqwest`.
- Public API changes go in `CHANGELOG.md`; the crate is published.
- Do not raise MSRV without a decision record.
- KBD state (`.kbd-orchestrator/*.json`) is runtime-owned: change it only through
  `prometheus kbd ...` commands. Blocking checks live in `.kbd-orchestrator/constraints.md`.
- Specs: OpenSpec in `openspec/` (changes via `openspec-propose` / `/opsx:*`).

