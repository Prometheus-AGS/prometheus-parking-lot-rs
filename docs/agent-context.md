# Agent context — prometheus_parking_lot

Project facts for agents. Read before changing code. `AGENTS.md` points here;
it stays small because the managed operating rules already fill its word budget.

## What it does

Tasks carry a `ResourceCost`. A `ResourcePool` or `WorkerPool` admits a task
through one dispatcher and a unit-weighted semaphore when its cost fits the free
units and nothing is parked ahead of it; otherwise it parks the task (priority,
then FIFO) and starts parked tasks in order as capacity frees. A task costing
more than the pool's total is rejected with `CapacityExceeded`. `ResourcePool`
delivers outcomes to a `Mailbox` so disconnected clients can retrieve them
later; `WorkerPool` holds them in result slots for `retrieve`. Executors return
`Result<T, TaskError>`. Shutdown, timeouts, cancellation capture and
idempotency follow the decisions recorded in
`.kbd-orchestrator/phases/phase-certification-remediation/decision-*.md` and
ADRs 0002 and 0003.

## Layout

Cargo workspace (ADR `docs/adr/0001-workspace-split.md`). The root is the
published facade `prometheus_parking_lot`; the layered crates are under
`crates/`.

```text
src/                          facade: re-exports, config/, builders/, runtime/api.rs, util/telemetry.rs
crates/parking-lot-core/      contracts/ (sync TaskQueue/Mailbox, async LeaseQueue/ResultMailbox,
                              conformance suite), dispatch/, semaphore/, hooks/, mailbox/, config/
                              — no async runtime, IO, database or network dependency
crates/parking-lot-runtime-tokio/  ResourcePool, native WorkerPool, shutdown, cancellation, TokioSpawner
crates/parking-lot-pg/        SeaORM PostgresQueue/PostgresMailbox, audit sink, migrations/, PG 17/18 test images
crates/parking-lot-embedded/  file-backed YaqueQueue/YaqueMailbox (redb in cr-28)
crates/parking-lot-wasm/      wasm32 WorkerPool (built for wasm32 only; cr-38)
tests/, benches/, examples/   full-stack tests, criterion (queue_bench), examples
docker/                       parking-lot-pg:18 and :17 images (Postgres + pgmq)
```

Facade features: `tokio-runtime` (default), `postgres`, `embedded`; the wasm
crate is selected by target. Core crate feature: `conformance`. Design
references: `docs/adr/`, `docs/API.md`, `README.md`, `docs/ARCHITECTURE.md`.

## Commands (mirror CI in `.github/workflows/ci.yml`)

```bash
cargo fmt    --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo nextest run --workspace --all-features --locked     # Docker required for Postgres tests
cargo test   --workspace --all-features --locked --doc    # includes the README examples
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked
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

