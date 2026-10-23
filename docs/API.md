# API overview

A map of the public API at 0.3.0 (unreleased). The rustdoc is the reference;
build it with `cargo doc --workspace --no-deps --all-features --open`. Compiled
examples are in the [README](../README.md). Design decisions are in
[`adr/`](adr/).

Most applications depend only on the facade crate `prometheus_parking_lot`.
Paths below are facade paths unless a crate is named.

## Pools

| Item | Path | Feature |
| --- | --- | --- |
| `ResourcePool<P, T, Q, M, E, S>`: async parking-lot scheduler over a `TaskQueue`, a `Mailbox`, a `TaskExecutor` and a `Spawn` | `core::ResourcePool` | `tokio-runtime` (default) |
| `WorkerPool<P, R, E>`: dedicated OS worker threads (native) for CPU/GPU-bound work over a `WorkerExecutor` | `WorkerPool`, `core::WorkerPool` | `tokio-runtime` on native; wasm32 by target |
| `PoolLimits { max_units, max_queue_depth, default_timeout }` | `core::PoolLimits` | always |
| `WorkerPoolConfig` (builder: `with_worker_count`, `with_max_units`, `with_max_queue_depth`, `with_timeout_ms`) | `config::WorkerPoolConfig` | always |
| `PoolStats` | `core::PoolStats` | always |
| `TokioSpawner` (`new(handle)`, `with_worker_threads(n)`) | `runtime::TokioSpawner` | `tokio-runtime` |
| `ShutdownHandle`, `ShutdownPolicy`, `ShutdownReport`, `wait_for_signal` | `core::shutdown` (`ShutdownReport` also top level) | `tokio-runtime` |
| `CancellationSink`, `CancelledTask`, `CancelReason`, `SinkOutcome`, `CancellationConfig`, the logging and recording sinks, `CancellationChannel` | `core::cancellation` | `tokio-runtime` |

`ResourcePool`: `new`, `with_audit`, `with_clock`, `with_cancellation_sink`,
`with_cancellation_config`, `submit`, `prune_expired`, `active_units`,
`running_tasks`, `queued_tasks`, `shutdown` (refuse new work only),
`shutdown_with(policy)` (async; returns a `ShutdownReport`, cr-29).

`DurableResourcePool` (crash-durable mode on a `LeaseQueue` and a
`ResultMailbox`, cr-49; top-level `DurableResourcePool`, `DurableOptions`):
`start(limits, queue, mailbox, executor, spawner, options)`, `submit` (returns
once the task is persisted), `queue`, `mailbox`, `options`, `active_units`,
`running_tasks`, `completed_tasks`, `failed_tasks`, `shutdown` (stop leasing
only), `shutdown_with(policy)` (async; hands cancelled tasks back to the queue
and returns a `ShutdownReport`, cr-29).
`DurableOptions::new(lease_for)` with `with_heartbeat_interval`,
`with_poll_interval`, `with_result_ttl`, `with_name`.

`WorkerPool`: `new`, `with_clock`, `with_result_ttl`, `with_cancellation_sink`,
`with_cancellation_config`, `submit` / `submit_async`, `retrieve` /
`retrieve_async`, `stats`, `shutdown` (`shutdown_with(Drain { deadline: 0 })`),
`shutdown_with(policy)` (blocking) / `shutdown_with_async(policy)`, both
returning a `ShutdownReport` (cr-29).

## Configuration

| Item | Path | Feature |
| --- | --- | --- |
| `SchedulerConfig { backends, pools }` (`from_json_str`, `validate`); every config struct rejects unknown fields | `config::SchedulerConfig` | always |
| `PoolConfig { max_units, max_queue_depth, default_timeout_secs, queue, mailbox }`, `StorageConfig` (`"in_memory"` or `{ "backend": name }`) | `config::*` | always |
| `BackendConfig` (`"type": "postgres"` / `"redb"`), `PostgresBackendConfig`, `RedbBackendConfig` | `config::*` | always (validation requires `postgres` / `embedded`) |
| `ConfigError` (`Parse` names an unknown or missing field; `Invalid { field, reason }` with a dotted path) | `config::ConfigError` | always |
| `build_pools_from_scheduler_config(cfg, executor_factory, spawner)` → `BuiltScheduler { pools, durable_pools, postgres, redb }` | `builders::*` | `tokio-runtime` |
| `ConfiguredDurablePool` (`Postgres` / `Redb` variants; `submit`, `consume`, `shutdown`) | `builders::*` | `postgres` or `embedded` |
| `ConfiguredPool { pool, mailbox }`, `SharedInMemoryMailbox` (`take`) | `builders::*` | `tokio-runtime` |
| `PostgresBackend { queue, mailbox, listener, lease_for }`, `RedbBackend { store, queue, mailbox, lease_for }` | `builders::*` | `postgres`, `embedded` |
| `build_pools(cfg, queue_factory, mailbox_factory, executor_factory, spawner)` | `builders::build_pools` | `tokio-runtime` |

A pool whose `queue` and `mailbox` are `"in_memory"` becomes a `ResourcePool`
(`BuiltScheduler::pools`); one whose `queue` and `mailbox` both name a backend
becomes a started `DurableResourcePool` on it (`BuiltScheduler::durable_pools`,
a `ConfiguredDurablePool` with `submit`, `consume` and `shutdown`; cr-49).
`build_pools` rejects durable pool sections. Example files: `examples/configs/`.

## Tasks and executors

| Item | Path |
| --- | --- |
| `TaskExecutor<P, T>`: `async fn execute(&self, P, TaskMetadata) -> Result<T, TaskError>` (serializable results) | `core::TaskExecutor` |
| `WorkerExecutor<P, R>`: same shape, result need not be serializable | `core::WorkerExecutor` |
| `TaskPayload` (blanket: `Send + Sync + Serialize + DeserializeOwned + 'static`) | `core::TaskPayload` |
| `TaskMetadata`, `ScheduledTask<P>`, `TaskStatus`, `SubmissionSequence` | `core::*` |
| `ResourceCost`, `ResourceKind`, `Priority`, `MailboxKey`, `TaskId` | `util::*` |
| `Clock`, `SystemClock`, `TestClock`, `now_ms` | `core::clock`, `util::*` |

## Errors

| Item | Covers |
| --- | --- |
| `SchedulerError` | Admission (`QueueFull`, `CapacityExceeded`, `DeadlineExpired`, `PoolShutdown`), `InvalidConfig`, `Backend`, `Timeout` (a wait for a result), `ResultNotFound`, `LeaseLost`, `Internal`, and `Task(TaskError)` |
| `TaskError` | How one task failed: `Executor { message, retryable }`, `ExecutorPanicked`, `Timeout { budget }`, `Cancelled { reason }`, `DeadlineExpired` |

Both are `#[non_exhaustive]`. See ADR 0002 for the mapping from the 0.2
`PoolError`.

## Storage contracts

Two generations coexist, per ADR 0003: the in-memory `ResourcePool` uses the
synchronous pair, `DurableResourcePool` (cr-49) the async pair.

| Contract | Kind | Implementations |
| --- | --- | --- |
| `TaskQueue<P>` | synchronous, in-process; what `ResourcePool` parks tasks in today | `infra::InMemoryQueue` |
| `Mailbox<T>` | synchronous; where `ResourcePool` delivers outcomes today | `infra::InMemoryMailbox` |
| `LeaseQueue<P>` | async, leasing (`enqueue`, `lease`, `ack`, `nack`, `extend`, `depth`) | `infra::queue::InMemoryLeaseQueue` (reference); `infra::queue::RedbLeaseQueue` from `infra::EmbeddedStore` (`embedded`, crash-safe); `infra::queue::PgLeaseQueue` (`postgres`) |
| `ResultMailbox<T>` + `Notifier<T>` | async, idempotent (`deliver`, `consume`, `peek`, `evict_expired`) | `infra::mailbox::InMemoryResultMailbox` (reference); `infra::mailbox::RedbResultMailbox` (`embedded`, crash-safe); `infra::mailbox::PgResultMailbox` (`postgres`); `StorageMailbox<S, N>` composes storage and notifier |

The async contracts live in `core::backend`; `DurableResourcePool` runs on them.
The backend conformance suite is `prometheus_parking_lot_core::conformance`
behind that crate's `conformance` feature.

## Hooks and audit

`core::hooks` (`Hook`, `HookBus`, `LifecycleEvent`, `TracingHook`,
`RecordingHook`) and `core::audit` (`AuditSink`, `AuditEvent`,
`InMemoryAuditSink`; `PostgresAuditSink` with `postgres`).

## Scheduling internals (core crate only)

`prometheus_parking_lot_core::dispatch` (`Dispatcher`, `Admission`,
`Dispatched`, `Grant`, `Ticket`, `Parkable`, `ParkedQueue`, `PriorityQueue`,
`TaskQueueAdapter`) and `prometheus_parking_lot_core::semaphore`
(`UnitSemaphore`, `Permit`) are what both pools admit through. The facade does
not re-export them.
