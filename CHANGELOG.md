# Changelog

All notable changes to `prometheus_parking_lot` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - Unreleased

0.3.0 is source-breaking. The design records are `docs/adr/0001-workspace-split.md`,
`docs/adr/0002-core-contracts.md` and `docs/adr/0003-backend-contracts.md`. Each
entry names the OpenSpec change that made it.

### Changed (breaking)

- **Workspace split (cr-48).** The crate is now a workspace: the facade
  `prometheus_parking_lot` re-exports `prometheus-parking-lot-core`,
  `-runtime-tokio`, `-pg`, `-embedded` and `-wasm` under the paths 0.2 used
  (`core::*`, `infra::*`, `config::*`, `runtime::*`, `util::*`). The embedded
  backends are behind the new `embedded` feature (0.2 compiled its file-backed
  stores unconditionally); since cr-28 that feature enables the redb backends
  below.
- **Toolchain (cr-41).** MSRV is 1.95 and the edition is 2024; the repository
  pins Rust 1.99.1 and commits `Cargo.lock`.
- **Fallible executors (cr-18).** `TaskExecutor::execute` and
  `WorkerExecutor::execute` return `Result<T, TaskError>`. `ResourcePool`
  delivers `TaskStatus::Completed` with the value or `TaskStatus::Failed(TaskError)`
  with no payload; `WorkerPool::retrieve` and `retrieve_async` return the error
  as `SchedulerError::Task`.
- **One error family (cr-18).** `PoolError` is removed; `WorkerPool` returns
  `SchedulerError`. `QueueFull` now carries `pool`, `depth`, `capacity` and
  `retry_hint`. `TaskError` (`Executor`, `ExecutorPanicked`, `Timeout`,
  `Cancelled`, `DeadlineExpired`) describes how one task failed and is
  serializable. `TaskStatus::Failed` carries a `TaskError` instead of a
  `String`. `TaskError`, `SchedulerError` and `TaskStatus` are
  `#[non_exhaustive]`. ADR 0002 has the full `PoolError` mapping.
- **Cost vector (cr-18).** `ResourceCost` is a vector of units per
  `ResourceKind`: build it with `ResourceCost::new(kind, units)` and `.with(..)`
  instead of a struct literal. It serializes as a map (`{"gpu_vram": 8}`) and
  still reads the 0.2 `{"kind": ..., "units": ...}` form. `ResourceKind` is
  `#[non_exhaustive]`. Pools still admit by `total_units()` against one
  `max_units`.
- **Submission order (cr-18).** `TaskMetadata` has a `sequence` field (set it
  to `0`; the pool stamps it), so equal-priority tasks run in submission order
  even within one millisecond.
- **Clock (cr-18).** `ResourcePool::submit` and `prune_expired` no longer take a
  `now_ms` argument; time comes from a `Clock` (`SystemClock` by default,
  `TestClock` for tests) set with `ResourcePool::with_clock`.
- **`ResourcePool` rejects tasks that can never fit (cr-18).** A task whose
  cost exceeds the pool's total capacity returns
  `SchedulerError::CapacityExceeded` at submission instead of being parked at
  the head of the queue, where it would block every task behind it.
  `sync_wake_worker_loop` and `WakeState` are removed.
- **Native `WorkerPool` admission (cr-19).** The native `WorkerPool` now
  enforces `max_units` (0.2 ignored it). Waiting tasks are served by priority,
  then in submission order. `submit` returns
  `SchedulerError::CapacityExceeded` for a task costing more than the pool's
  total units and `SchedulerError::DeadlineExpired` for a task whose deadline
  has passed. At most `worker_count` tasks run at once and `max_queue_depth`
  bounds the tasks waiting. Tasks still waiting at `shutdown` are failed with
  `TaskError::Cancelled` instead of being dropped.
- **`WorkerPool` execution budget (cr-19).** Every `WorkerPool` execution is
  bounded by the task's deadline or `WorkerPoolConfig::default_timeout_ms`,
  whichever is nearer. `default_timeout_ms` is therefore now also the
  execution budget, not only the default wait for `retrieve`. A task still
  running at its budget is cancelled, its units are released, and `retrieve`
  returns `SchedulerError::Task(TaskError::Timeout { .. })`. Raise
  `default_timeout_ms` (or set a per-task `deadline_ms`) to the longest run
  you intend to permit.
- **Executor panics are contained (cr-21).** A panic in an executor run by
  `ResourcePool` or the native `WorkerPool` no longer escapes: the task's
  outcome is `TaskError::ExecutorPanicked`, its units are released and parked
  work is started. A `WorkerPool` worker thread killed by a panic is replaced.
- **`WorkerPool::stats().worker_count` reports live workers (cr-21).** It now
  counts the worker threads serving the pool instead of echoing the configured
  count, and drops to 0 after `shutdown`.
- **Blocking `WorkerPool::retrieve` refuses async threads (cr-22).** It now
  panics when called on a thread that is driving async tasks; call it from a
  plain thread or `spawn_blocking`, or use `retrieve_async`.
- **A timed-out retrieve keeps the result (cr-22).** A `retrieve` or
  `retrieve_async` that times out leaves the result in its slot, so a later
  call can still take it until the result TTL evicts it.
- **Strict scheduler configuration (cr-37).** Every config struct, including
  `WorkerPoolConfig`, rejects unknown fields, and the error names the field.
  `SchedulerConfig` gains named `backends` (`"type": "postgres"` or `"redb"`,
  with their parameters) and `pools` is a `BTreeMap`. `PoolConfig.queue` and
  `.mailbox` are `"in_memory"` or `{ "backend": name }`; naming a backend is
  rejected until durable dispatch (cr-49). The ignored `runtime` field and
  `RuntimeConfig`, `QueueBackendConfig` and `MailboxBackendConfig` are
  removed. `validate` and `from_json_str` return `ConfigError` (`Parse` names
  an unknown or missing field; `Invalid` names the field by dotted path, e.g.
  `pools.inference.max_units`) instead of `String`; `build_pools` returns a
  `BTreeMap`. `WorkerPoolConfig::thread_stack_size` exists on every target
  (ignored on WASM), and `default_timeout_ms` is documented as the execution
  budget (and the native default result TTL), not a `retrieve` timeout.

### Added
- **Pools and backends from configuration (cr-37).**
  `builders::build_pools_from_scheduler_config` validates a `SchedulerConfig`,
  opens every backend (Postgres: connect, optional migration of the queue and
  mailbox schemas, `PgLeaseQueue`, `PgResultMailbox` and an optional
  `QueueListener`; redb: `EmbeddedStore`, `RedbLeaseQueue`,
  `RedbResultMailbox`) and builds every pool with an in-memory queue and a
  readable `SharedInMemoryMailbox`. One example per backend is in
  `examples/configs/`, and the test suite loads and builds each of them, plus
  every JSON config block in the README and docs.

- **Dispatcher and unit-weighted semaphore (cr-19).** Both pools now admit
  through one `Dispatcher` that owns a `UnitSemaphore` and the parked queue, so
  admission, parking and waking happen under one lock with no lost wakeup.
  New in `prometheus_parking_lot_core::dispatch`: `Dispatcher`, `Admission`,
  `Dispatched`, `Grant`, `Ticket`, `Parkable`, `ParkedQueue`, `PriorityQueue`,
  `TaskQueueAdapter`. New in `prometheus_parking_lot_core::semaphore`:
  `UnitSemaphore`, `Permit`. The facade does not re-export these; applications
  use the pools.
- `ResourcePool::running_tasks` and `ResourcePool::queued_tasks` (cr-19).
- `ResourcePool::completed_tasks` and `ResourcePool::failed_tasks` (cr-21):
  counts of tasks that returned a value and of tasks whose outcome was a
  failure (executor error or panic, timeout, or a deadline that passed while
  parked).
- `WorkerPool::with_clock` (cr-19): drive deadline checks and execution
  budgets from a `TestClock`.
- `WorkerPool::with_result_ttl(Duration)` (cr-22): results nobody retrieves
  are evicted after the TTL (default: the pool's `default_timeout_ms`), and a
  later retrieve reports `SchedulerError::ResultExpired`.
- `SchedulerError::ResultExpired` (cr-22): a stored result was not retrieved
  within its TTL and was evicted; not retryable.
- **Async backend contracts (cr-25).** `LeaseQueue` (`enqueue`, `lease`, `ack`,
  `nack`, `extend`, `depth`, `max_depth`) with `LeaseToken`, `LeasedTask`,
  `NackAction`, `Enqueued`, `QueueDepth` and `QueueEntryId`; `ResultMailbox`
  (`deliver`, `consume`, `peek`, `evict_expired`) with `Delivery`,
  `DeliveryOutcome` and `MailboxEntry`; `Notifier` with `NoopNotifier`; and
  `StorageMailbox<S, N>`, which pairs storage with a notifier. They are
  re-exported by the facade under `core::backend`. The reference
  implementations are `InMemoryLeaseQueue` (with `DeadLetter`) in
  `infra::queue` and `InMemoryResultMailbox` in `infra::mailbox`. No pool uses
  these contracts yet (durable dispatch is cr-49). The redb backends (cr-28,
  below) implement them; the Postgres ones are cr-26 and cr-27.
- `SchedulerError::LeaseLost` (cr-25): the lease on a queue entry is no longer
  held; not retryable.
- **Backend conformance suite (cr-25).** The `conformance` feature of
  `prometheus-parking-lot-core` publishes `conformance`,
  `lease_queue_conformance!` and `result_mailbox_conformance!`, the tests every
  `LeaseQueue` and `ResultMailbox` backend must pass.
- **Postgres test images (cr-43).** `docker/postgres18` and `docker/postgres17`
  build `parking-lot-pg:18` (PostgreSQL 18.6) and `parking-lot-pg:17`
  (PostgreSQL 17.11) from digest-pinned official images, with pgmq 1.13.1
  checked against its commit. `docker-compose.test.yml`, the
  `crates/parking-lot-pg` testcontainers helper and `scripts/test-postgres.sh`
  use them; the Docker-only (`#[ignore]`d) smoke tests check that each image
  starts and has pgmq. These are
  test and development images, not part of the published crates.

- **Postgres lease queue (cr-26).** `prometheus-parking-lot-pg::PgLeaseQueue` is
  an async `LeaseQueue` on raw sqlx 0.9 with no ORM and no `block_on`. It leases
  with `FOR UPDATE SKIP LOCKED` in priority-then-FIFO order, fences leases with
  the attempt count (a stale token gets `LeaseLost`), supports nack with delay,
  dead-lettering and `with_max_attempts`, keeps idempotency keys first-write-wins,
  and wakes idle dispatchers through LISTEN/NOTIFY (`PgLeaseQueue::listener()`,
  `QueueListener`). `reap_expired` and `spawn_reaper` return expired leases.
  Migration `004_lease_queue.sql` adds the `pl_lease_queue` table, and
  `migrate(pool)` applies it. No pool uses it yet (durable dispatch is cr-49).
- **Postgres result mailbox (cr-27).** `prometheus-parking-lot-pg::PgResultMailbox`
  is an async `ResultMailbox` on raw sqlx 0.9. Consume is one `DELETE ... RETURNING`
  with `FOR UPDATE SKIP LOCKED`, so a result reaches exactly one consumer.
  Duplicate deliveries keep the first write via `INSERT ... ON CONFLICT`. TTLs use
  database time, with `evict_expired` and a background sweeper (`spawn_sweeper`).
  Migration `005_result_mailbox.sql`; `infra::PgPool` is re-exported.
- **Removed:** the synchronous sea-orm `PostgresQueue` and `PostgresMailbox`, the
  sea-orm entities (`infra::entities`), and migrations 001 and 002 (existing
  `pl_queue_jobs` / `pl_mailbox_messages` tables are not dropped). sea-orm and
  sqlx 0.8 are gone from the dependency graph; the `postgres` feature now uses
  raw sqlx 0.9 only.

- **Crash-safe embedded queue and mailbox (cr-28, ADR 0004).** With the
  `embedded` feature, `EmbeddedStore::open(path)` opens one redb database file;
  `store.lease_queue::<P>(max_depth)` returns a `RedbLeaseQueue` and
  `store.result_mailbox::<T>()` a `RedbResultMailbox`. They implement the async
  `LeaseQueue` and `ResultMailbox` contracts and pass the conformance suite.
  Every operation is one redb write transaction committed durably before it
  returns, so an accepted task survives a crash, an expired lease is
  redelivered and an acked task never returns. They need a tokio runtime, and
  one process holds a database file at a time. The facade re-exports them as
  `infra::EmbeddedStore`, `infra::queue::RedbLeaseQueue` and
  `infra::mailbox::RedbResultMailbox`. No pool can use them yet: the pools
  still park and deliver through the synchronous `TaskQueue` and `Mailbox`
  until durable dispatch (cr-49). `examples/embedded_pool.rs` shows direct use.

- **WASM pool on the shared dispatcher (cr-38).** The WASM `WorkerPool` now
  admits through the core dispatcher and runs on
  `wasm_bindgen_futures::spawn_local` with JS timers; it no longer depends on
  tokio and builds and passes `wasm-bindgen-test` on `wasm32-unknown-unknown`.
  Tasks past their deadline or `default_timeout_ms` are cancelled and fail
  with `TaskError::Timeout`. New: `WorkerPool::{submit, with_clock}`,
  `WasmClock` and `sleep`. Executor panics abort the module on
  `wasm32-unknown-unknown`; native panic containment does not apply there.
  CI gains a wasm check/test job and a core/wasm dependency-graph gate.

- **Mobile and desktop bindings (cr-44).** New crate `prometheus-parking-lot-ffi`
  (not published) exposes the embedded redb queue and result mailbox to Kotlin and
  Swift through UniFFI 0.32: `open_store`, then async `enqueue`, `lease`, `ack`,
  `nack`, `extend_lease`, `depth`, `deliver_result`, `consume_result`,
  `peek_result` and `evict_expired_results`, with payloads as bytes. It builds as
  `lib`, `cdylib` (Android) and `staticlib` (iOS) and contains no hand-written
  `unsafe`. `bindings/generate.sh` generates the Kotlin and Swift sources in
  library mode (nothing generated is committed); `bindings/smoke/` runs each
  against the real library. A Tauri 2 desktop example (`bindings/tauri-desktop`,
  its own workspace) keeps its queue across an app restart; the restart is tested
  without a GUI. CI gains a `Platform` workflow (Android via `cargo ndk`, iOS
  device and simulator, Kotlin and Swift smoke tests, the Tauri example), and the
  `deny` job now checks the whole workspace (`--workspace`; before, it only saw
  the facade's dependency graph). `deny.toml` allows MPL-2.0 for the UniFFI crates
  only. See `bindings/README.md`.

### Removed

- **The JSONL file queue and mailbox (cr-28).** `YaqueQueue`, `YaqueMailbox`
  and the facade paths `infra::queue::yaque` and `infra::mailbox::yaque` are
  removed; the redb backends replace them. They were not crash-safe, and their
  files are not migrated. `examples/yaque_pool.rs` is now
  `examples/embedded_pool.rs`.

### Fixed

- `WorkerPool::retrieve_async` no longer uses a blocking-pool thread per
  waiter (cr-22); any number of concurrent waiters fit on a runtime whose
  blocking pool is exhausted.
- `TokioSpawner::with_worker_threads` keeps its runtime alive for as long as
  the spawner or any clone exists (cr-19). In 0.2 the runtime was dropped when
  the constructor returned, so every spawn went to a dead runtime.
  `TokioSpawner` now also implements `Debug`.

- The WASM pool ignored `max_units` (audit C1); it now enforces capacity,
  priority then FIFO order and the queue depth (cr-38).

### Known limitations

- No pool runs on the Postgres backends yet: `PgLeaseQueue` and `PgResultMailbox`
  are called directly until durable dispatch (cr-49).

## [0.2.0]

### Fixed

- **Execution budgets are now enforced.** `PoolLimits::default_timeout` was
  configured, validated (`config/pool.rs`) and plumbed through the builder
  (`builders/pool_builder.rs`), but was **never read in the execution path**:
  both `ResourcePool::spawn_task` and the wake path awaited
  `executor.execute(..)` unbounded. A task that never returned held its
  capacity units for the life of the process, so the pool's central capacity
  guarantee did not hold — one wedged task could starve a single-unit pool
  permanently.

  Execution is now wrapped in `tokio::time::timeout` on **both** paths. On
  elapse the executor future is dropped, which cancels it at its next await
  point; an executor owning a `tokio::process::Child` built with
  `kill_on_drop(true)` gets its child reaped as part of that drop.

- A per-task `TaskMetadata::deadline_ms` nearer than the pool default now
  bounds that task, via `ResourcePool::effective_budget`. An already-passed
  deadline yields a near-zero budget instead of saturating to the default.

- A timed-out task is delivered to the mailbox as
  `TaskStatus::Failed("execution exceeded its timeout budget")` with a `None`
  payload, and audited as `timeout` rather than `complete`. Previously such a
  task could never reach the mailbox at all.

### Changed

- **(internal)** `ResourcePool::on_task_finished_static` takes `Option<T>`
  rather than `T`, since a cancelled task has no result value. This is a
  private associated function; the public API is unchanged.

### Notes for upgraders

No source changes are required. Behaviour changes in one way that matters:
work exceeding `default_timeout` is now **cancelled** instead of running
indefinitely. If a pool was relying on the previous unbounded behaviour, raise
`default_timeout` (or set a per-task `deadline_ms`) to the longest run you
actually intend to permit. The default is defined in `config/pool.rs` and
validated as non-zero.

## [0.1.0]

- Initial release: `ResourcePool` parking-lot scheduler, `WorkerPool`,
  in-memory / SeaORM / pgmq / Yaque queue backends, in-memory / SeaORM /
  HTTP-callback mailbox backends, JSON/YAML configuration.
