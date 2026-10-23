# ADR 0001: Split the crate into a layered Cargo workspace

## Status

Accepted (cr-48-workspace-split, phase-correctness-and-durability, milestone M0).

## Date

2026-10-22

## Context

`prometheus_parking_lot` was one crate. Every build compiled tokio, SeaORM/sqlx and the
file-backed backends together, so a one-line change to a contract rebuilt the Postgres
stack, and the "pure" scheduling core could not be built for `wasm32` without tokio.
Four implementers (scheduler, Postgres, events/embedded, platform) are about to work in
parallel; in one crate they would share files and rebase across each other.

Requirements (openspec `crate-structure`):

- the core crate builds with no async runtime, IO, database or network crate, so it can
  target `wasm32-unknown-unknown` (cr-38 makes that build real);
- the published facade keeps the name `prometheus_parking_lot` and default-feature users
  still reach the core types through it;
- the split is mechanical: no behaviour change, done once, before fan-out.

## Decision

### Crate map

| Directory | Package (published name) | Holds | Owner role |
|---|---|---|---|
| `.` (root) | `prometheus_parking_lot` (facade) | re-exports; `config/` (scheduler, pool and backend selection config); `builders/`; `runtime/api.rs`; `util/telemetry.rs`; full-stack tests, benches, examples | tech-lead (`runtime/` platform-engineer) |
| `crates/parking-lot-core` | `prometheus-parking-lot-core` | `contracts/` (TaskQueue, Mailbox, Spawn, TaskExecutor/WorkerExecutor/TaskPayload, TaskMetadata/ScheduledTask/TaskStatus/PoolLimits, Priority/ResourceCost/ResourceKind/MailboxKey/TaskId, clock, SchedulerError, PoolError/PoolStats, the in-memory queue); `config/` (WorkerPoolConfig); `dispatch/`; `semaphore/`; `hooks/` (hooks + audit); `mailbox/` (in-memory mailbox) | tech-lead (contracts, config, lib.rs), scheduler-engineer (dispatch, semaphore), events-engineer (hooks, mailbox) |
| `crates/parking-lot-runtime-tokio` | `prometheus-parking-lot-runtime-tokio` | `ResourcePool`, `WakeState`, `sync_wake_worker_loop`, native `WorkerPool`, `shutdown`, `cancellation`, `TokioSpawner` | scheduler-engineer |
| `crates/parking-lot-pg` | `prometheus-parking-lot-pg` | `PostgresQueue`, `PostgresMailbox`, SeaORM entities, `PostgresAuditSink`, `migrations/` | postgres-engineer |
| `crates/parking-lot-embedded` | `prometheus-parking-lot-embedded` | the file-backed `YaqueQueue` / `YaqueMailbox` (replaced by redb in cr-28) | events-engineer |
| `crates/parking-lot-wasm` | `prometheus-parking-lot-wasm` | the `wasm32` `WorkerPool` | platform-engineer |

`parking-lot-iggy` and `parking-lot-ffi` are not created here; cr-31/cr-32 and cr-44
create them, with their own features, when they have code.

*Amendment (cr-44):* `crates/parking-lot-ffi` now exists as `prometheus-parking-lot-ffi`
(not published): UniFFI Kotlin/Swift bindings over the embedded backend, owned by
platform-engineer. The binding generator and the Tauri example live in standalone
workspaces under `bindings/`, outside this workspace.

### Dependency rules

1. **core** depends on no async runtime, IO, database or network crate. Its normal
   dependencies are `anyhow`, `async-trait`, `parking_lot`, `serde`, `thiserror`,
   `tracing`, and `num_cpus` (non-wasm only, for the `WorkerPoolConfig` default). The
   blocking constraint `core-has-no-infra-deps` checks it with
   `cargo tree -p prometheus-parking-lot-core -e normal` (no tokio, sqlx, sea-orm, iggy,
   redb, reqwest).
2. **runtime-tokio, pg, embedded, wasm** depend on core and never on each other.
3. **facade** depends on core always and on the others behind features (wasm by target).
4. Crates never depend on the facade, not even as a dev-dependency (it would create a
   second copy of every type in tests).
5. Third-party version requirements are written once in `[workspace.dependencies]`;
   member crates inherit them and add only features.

### Feature mapping (facade)

| Feature | Enables | Default |
|---|---|---|
| `tokio-runtime` | `prometheus-parking-lot-runtime-tokio` (ResourcePool, native WorkerPool, shutdown, cancellation, TokioSpawner, `builders`, `runtime`) | yes |
| `postgres` | `prometheus-parking-lot-pg` and the direct optional `sea-orm` dependency (applications open the `DatabaseConnection` the backends take) | no |
| `embedded` | `prometheus-parking-lot-embedded` | no |
| target `wasm32` | `prometheus-parking-lot-wasm` (provides `core::WorkerPool` on wasm32) | by target |

Member crates have no features. Features exist only for crates that exist.

### Naming

Directories are short (`crates/parking-lot-*`); every package is named
`prometheus-parking-lot-*`. crates.io treats `-` and `_` as the same name, and
`parking-lot-core` / `parking_lot_core` belongs to the `parking_lot` project (checked
2026-10-22), so an unprefixed name would collide. The facade keeps its published name
`prometheus_parking_lot`.

### Toolchain, version and lints

- `[workspace.package]`: `version = "0.2.0"` (0.3.0 is set at release, cr-52),
  `edition = "2024"`, `rust-version = "1.95"` (MSRV; the toolchain stays pinned to
  1.99.1 by `rust-toolchain.toml`), plus shared authors/repository/homepage/license.
- `[workspace.lints]` carries the cr-41 set unchanged and every crate opts in with
  `[lints] workspace = true`: `unsafe_code = "deny"`, `missing_docs = "deny"`,
  clippy `all`/`pedantic`/`nursery = "warn"`, and the `unexpected_cfgs` check-cfg entries
  `cfg(parking_lot_loom)` and `cfg(parking_lot_negative_control, values(any()))`.
- The cr-41 `#[expect]` attributes moved with their code with their reasons unchanged
  (30 remain). The one removed (`clippy::new_without_default` on
  `PostgresMailbox::new()`) sat on the non-`postgres` stub constructor, which no longer
  exists (see below).

### Packaging

Each package has an anchored `include` allowlist instead of the old root `exclude` list:
`/src/**`, `/Cargo.toml`, `/README.md`, `/LICENSE*`, plus `/migrations/**` for pg and
`/CHANGELOG.md` for the facade. Patterns are anchored with `/` because an unanchored
`README.md` or `LICENSE*` matches at any depth (it pulled `tests/integration/README.md`
and every `.agents/skills/*/LICENSE` into the packages).

### Placement decisions

- **In-memory queue in `contracts/`** (`contracts::memory_queue::InMemoryQueue`), not a
  new `queue/` module: it is the reference implementation of the `TaskQueue` contract
  that cr-25's conformance suite runs against, and cr-25 (tech-lead) owns both. Ownership
  of this file moves from events-engineer to tech-lead.
- **Value types and clock in `contracts/`** (`contracts::types`, `contracts::clock`), not
  a core `util/`: they are the contract's vocabulary (task id, priority, cost, mailbox
  key), and cr-18 turns the clock into a `Clock` contract. The facade keeps
  `util::serde` and `util::clock` as re-exports.
- **`WorkerPoolConfig` in core `config/`**: both worker pool implementations
  (runtime-tokio and wasm) are built from it. `SchedulerConfig`, `PoolConfig` and the
  backend-selection enums stay in the facade because they wire backends.
- **Worker-pool shared internals in `dispatch/`**: `PoolCounters`, `WorkerTask`,
  `generate_mailbox_key`, `mailbox_key_to_string` were `pub(crate)` and shared by the
  native and wasm pools. They are now `pub` + `#[doc(hidden)]` so both crates can use
  them, and are documented as not a stable API; cr-19's dispatcher replaces them.
  `PoolError` and `PoolStats` are contract types in `contracts::worker_pool`.
- **`semaphore/`** is an empty, documented module for cr-19.
- **`hooks/audit.rs`** keeps `AuditEvent`, `AuditSink`, `InMemoryAuditSink`,
  `build_audit_event`; **`PostgresAuditSink` moves to pg** (`audit.rs`).
- **`cancellation` goes to runtime-tokio**: its channel and drain task use tokio `mpsc`
  and `tokio::time::timeout`. Nothing tokio-dependent was left in core, and no behaviour
  was weakened to make core compile.
- **`runtime/api.rs` stays in the facade**: it is an API-surface skeleton that needs both
  `ResourcePool` and the facade's `SchedulerConfig`. Only `TokioSpawner` moved to
  runtime-tokio; `runtime::tokio_spawner` is re-exported.
- **`util/telemetry.rs` stays in the facade**: it installs a `tracing-subscriber`
  subscriber, which a library core should not depend on.
- **wasm crate**: the pool is compiled only for `target_arch = "wasm32"`, exactly as the
  module was before; on the host the crate is empty and its dependencies are
  target-gated. Making it build for wasm32 is cr-38.
- **pg crate has no feature gates**: the old `#[cfg(feature = "postgres")]` /
  `#[cfg(not(feature = "postgres"))]` branches are resolved to the postgres side. The
  non-postgres stubs (constructors without a connection and `TaskQueue`/`Mailbox` impls
  that always returned "feature disabled") are deleted; without the facade's `postgres`
  feature the Postgres types are simply absent.
- **Migrations** move to `crates/parking-lot-pg/migrations/`;
  `docker-compose.test.yml` mounts the new path.

### Tests

- Unit tests (`#[cfg(test)]`) moved with their modules: core 21, runtime-tokio 12 (the
  same 33 the single crate ran).
- Integration tests moved to the crate that owns the code under test, with imports
  rewritten to the crate paths: `worker_pool_test`, `parking_lot_algorithm_test`,
  `execution_timeout_test`, `candle_vllm_suite` (+ `candle_vllm/`) and
  `llm_inference_test` to `crates/parking-lot-runtime-tokio/tests/`.
- `tests/restart_resume.rs` (full-stack placeholder), `benches/queue_bench.rs` and the
  examples stay at the root and use the facade paths.
- The `tests/unit/*` and `tests/integration/*` suites were never registered test targets
  (subdirectories are not auto-discovered); they moved unchanged next to their code
  (`crates/*/tests/unit/`, `crates/parking-lot-pg/tests/integration/`) and remain
  unregistered. cr-35 registers and ports them. `config_test` and `builders_test` stay
  in the root `tests/unit/`.

### Public paths

Kept (with default features unless noted): every `prometheus_parking_lot::core::*`,
`core::{audit, cancellation, error, executor, hooks, resource_pool, shutdown,
worker_pool}::*`, `config::*` (including `config::pool::WorkerPoolConfig`),
`builders::*`, `runtime::{api, tokio_spawner, TokioSpawner, ...}`,
`util::{clock, serde, telemetry}::*` and the `util::*` globs,
`infra::{queue, mailbox}::memory::*`, `infra::{InMemoryQueue, InMemoryMailbox}`, and
the crate-root re-exports.

Changed:

| Path | Now |
|---|---|
| `infra::queue::yaque::*`, `infra::mailbox::yaque::*`, `infra::{YaqueQueue, YaqueMailbox}`, `infra::{queue, mailbox}::{YaqueQueue, YaqueMailbox}` | require feature `embedded` (were always compiled) |
| `infra::queue::postgres::PostgresQueue`, `infra::mailbox::postgres::PostgresMailbox`, `infra::{queue, mailbox}::{PostgresQueue, PostgresMailbox}` | require feature `postgres` (a non-functional stub existed without it) |
| `PostgresQueue::new(queue_name, max_depth)` and `PostgresMailbox::new()` (stub constructors) | removed; only the `DatabaseConnection` constructors exist |
| `core::PostgresAuditSink`, `core::audit::PostgresAuditSink` | require feature `postgres` |
| `builders`, `runtime`, `core::{ResourcePool, WorkerPool, cancellation, shutdown, ...}` | require `tokio-runtime` (default). Before the split the crate did not compile without it, so no working build loses them |
| implicit features `tokio`, `tokio-util`, `sea-orm`, `sqlx`, `chrono` | removed; only `tokio-runtime`, `postgres`, `embedded` remain (`dep:` syntax) |

New paths: the member crates themselves (`prometheus_parking_lot_core::...`, etc.).

### Other observable changes

- **`tracing` targets**: events emitted by moved code now carry the member crate's module
  path (for example `prometheus_parking_lot_runtime_tokio::worker_pool::native`). A filter
  such as `RUST_LOG=prometheus_parking_lot=debug` no longer matches them; add
  `prometheus_parking_lot_core` and `prometheus_parking_lot_runtime_tokio`.
- **Facade dependencies**: unused direct dependencies were dropped (`lock_api`,
  `parking_lot_core`, `dotenvy`, `uuid`, `anyhow`, `thiserror`, `async-trait`, `parking_lot`, `num_cpus`,
  `crossbeam-channel`, `tokio`, `tokio-util`, `sqlx`, `chrono` move to the crates that use
  them or are gone). Dev-dependencies used only by unregistered suites (`tempfile`,
  `testcontainers`, `testcontainers-modules`) and the unused `rand` were dropped;
  `Cargo.lock` lost only packages, no retained package changed version.

## Alternatives considered

- **Stay a single crate with feature gates.** Rejected: features do not shrink rebuild
  scope for contract edits, cannot keep tokio out of the core when the facade's default
  enables it, and leave four implementers editing one crate.
- **Unprefixed package names (`parking-lot-core`, ...).** Rejected: collides with the
  `parking_lot` project's `parking_lot_core` on crates.io.
- **Facade as a separate `crates/` member with a virtual root manifest.** Rejected for
  now: the root package keeps `cargo publish` from the repository root, the README,
  CHANGELOG and examples where users expect them, and existing tooling paths.
- **Keep the non-postgres stubs.** Rejected: a type that compiles but always fails at
  runtime is worse than an absent type, and keeping it required a `postgres` feature on
  the pg crate whose only meaning would be "be real".

## Consequences

- Each implementer owns a crate (or core module); contract edits rebuild core and its
  dependents, not unrelated backends.
- Commands without `--workspace` at the repository root now check only the facade
  package. CI and the blocking constraints that still run `cargo check|clippy|test|doc`
  without `--workspace` must add it (follow-up below).
- Publishing order is core, then runtime-tokio/pg/embedded/wasm, then the facade
  (cr-52).

## Follow-ups

- **Operator / platform-engineer (cr-42):** add `--workspace` to the `build-passes`,
  `clippy-clean`, `tests-pass` and `docs-build` constraint commands and CI jobs; extend
  `no-hardcoded-secrets`, `no-unwrap-in-library-code` and `no-stub-comments` greps from
  `src/` to `src/ crates/`; update the per-role aliases in `.cargo/config.toml`
  (`ck-pg` etc. now need `-p` or `--workspace`).
- **platform-engineer (cr-51):** check `cargo package --list` for every published crate
  against the allowlists above; decide how the MIT license text ships with member crates
  (none has a `LICENSE*` file in its directory yet).
- **scheduler-engineer (cr-19/cr-22):** `WorkerPool::retrieve_async` races its inner
  `condvar.wait_for(timeout)` against the outer `tokio::time::timeout(timeout)`; when the
  inner wait wins it returns `ResultNotFound` instead of `Timeout`, so
  `worker_pool_test::test_timeout_handling` fails intermittently (about 1 run in 7 here).
  Pre-existing; the code moved byte-for-byte. Also out of scope here:
  `TokioSpawner::with_worker_threads` drops its `Runtime`.
- **postgres-engineer (cr-26/cr-27/cr-35):** the direct `sqlx` dependency (with `macros`)
  is kept on the pg crate exactly as the facade had it, although the SeaORM code does not
  name `sqlx`; the raw-sqlx rewrite uses it.
- **product-manager (cr-16):** README and docs still describe `src/core`, `src/infra` and
  the pgmq extension; update the architecture section to this crate map.
- **tech-lead:** update `.agent-team/parking-lot-core/team.json` ownership for the moved
  files (in-memory queue to tech-lead; `TokioSpawner` to scheduler-engineer via
  `crates/parking-lot-runtime-tokio/src/**`; core unit suites under
  `crates/parking-lot-core/tests/unit/` belong to their modules' owners).
