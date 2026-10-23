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
| `ShutdownHandle`, `ShutdownPolicy`, `wait_for_signal` | `core::shutdown` | `tokio-runtime` |
| Cancellation channel and sinks | `core::cancellation` | `tokio-runtime` |

`ResourcePool`: `new`, `with_audit`, `with_clock`, `submit`, `prune_expired`,
`active_units`, `running_tasks`, `queued_tasks`, `shutdown`.

`WorkerPool`: `new`, `with_clock`, `submit` / `submit_async`, `retrieve` /
`retrieve_async`, `stats`, `shutdown`.

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

Two generations coexist until durable dispatch (cr-49), per ADR 0003.

| Contract | Kind | Implementations |
| --- | --- | --- |
| `TaskQueue<P>` | synchronous, in-process; what `ResourcePool` parks tasks in today | `infra::InMemoryQueue` |
| `Mailbox<T>` | synchronous; where `ResourcePool` delivers outcomes today | `infra::InMemoryMailbox` |
| `LeaseQueue<P>` | async, leasing (`enqueue`, `lease`, `ack`, `nack`, `extend`, `depth`) | `infra::queue::InMemoryLeaseQueue` (reference); `infra::queue::RedbLeaseQueue` from `infra::EmbeddedStore` (`embedded`, crash-safe); `infra::queue::PgLeaseQueue` (`postgres`) |
| `ResultMailbox<T>` + `Notifier<T>` | async, idempotent (`deliver`, `consume`, `peek`, `evict_expired`) | `infra::mailbox::InMemoryResultMailbox` (reference); `infra::mailbox::RedbResultMailbox` (`embedded`, crash-safe); `infra::mailbox::PgResultMailbox` (`postgres`); `StorageMailbox<S, N>` composes storage and notifier |

The async contracts live in `core::backend` and are not yet used by any pool.
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
