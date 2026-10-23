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
  (`core::*`, `infra::*`, `config::*`, `runtime::*`, `util::*`). The file-backed
  `YaqueQueue` and `YaqueMailbox` are now behind the new `embedded` feature
  (0.2 compiled them unconditionally).
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

### Added

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
- **Async backend contracts (cr-25).** `LeaseQueue` (`enqueue`, `lease`, `ack`,
  `nack`, `extend`, `depth`, `max_depth`) with `LeaseToken`, `LeasedTask`,
  `NackAction`, `Enqueued`, `QueueDepth` and `QueueEntryId`; `ResultMailbox`
  (`deliver`, `consume`, `peek`, `evict_expired`) with `Delivery`,
  `DeliveryOutcome` and `MailboxEntry`; `Notifier` with `NoopNotifier`; and
  `StorageMailbox<S, N>`, which pairs storage with a notifier. They are
  re-exported by the facade under `core::backend`. The reference
  implementations are `InMemoryLeaseQueue` (with `DeadLetter`) in
  `infra::queue` and `InMemoryResultMailbox` in `infra::mailbox`. No pool uses
  these contracts yet (durable dispatch is cr-49), and no durable backend
  implements them yet (cr-26, cr-27, cr-28).
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

### Fixed

- `TokioSpawner::with_worker_threads` keeps its runtime alive for as long as
  the spawner or any clone exists (cr-19). In 0.2 the runtime was dropped when
  the constructor returned, so every spawn went to a dead runtime.
  `TokioSpawner` now also implements `Debug`.

### Known limitations

- The Postgres queue still removes a task when it dequeues it (at-most-once)
  and bridges async calls with `block_on`; the leasing Postgres backends are
  cr-26 and cr-27.

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
