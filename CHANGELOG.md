# Changelog

All notable changes to `prometheus_parking_lot` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
