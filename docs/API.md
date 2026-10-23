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
| `WorkerPoolConfig` (builder: `with_worker_count`, `with_max_units`, `with_max_queue_depth`, `with_timeout_ms`, `with_max_head_skips`, `with_resource_limit`; field `resource_limits`) | `config::WorkerPoolConfig` | always |
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
| `PoolConfig { max_units, max_queue_depth, default_timeout_secs, queue, mailbox, max_head_skips, resource_limits }`, `StorageConfig` (`"in_memory"` or `{ "backend": name }`) | `config::*` | always |
| `BackendConfig` (`"type": "postgres"` / `"redb"` / `"pgmq"` / `"iggy"`), `PostgresBackendConfig`, `RedbBackendConfig`, `PgmqBackendConfig`, `IggyBackendConfig` (`options()` with `iggy`) | `config::*` | always (validation requires `postgres` / `embedded` / `iggy`) |
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
`StorageMailbox::new(storage, notifier)` takes a `DetachedNotifier`:
`NoopNotifier`, or `BackgroundNotifier::new(notifier, spawner, sleeper)`, which
runs any `Notifier` off the delivery path with bounded retries (`with_policy(RetryPolicy)`,
`stats()` returning `NotifyStats` / `NotifyStatsSnapshot`; cr-34). All of them
are in `core::backend` and at `core::*`.
The backend conformance suite is `prometheus_parking_lot_core::conformance`
behind that crate's `conformance` feature.

## Hooks and audit

`core::hooks` (`Hook`, `HookBus`, `LifecycleEvent`, `TracingHook`,
`RecordingHook`) and `core::audit` (`AuditSink`, `AuditEvent`,
`InMemoryAuditSink`; `PostgresAuditSink` with `postgres`).

`ResourcePool`, the native `WorkerPool` and `DurableResourcePool` emit, per
task, `Submitted`, then `Queued` if it parks, `Started` when the executor is
about to run, and exactly one terminal event: `Completed`, `Failed`,
`Rejected`, `Cancelled`, `Expired` or (durable pool) `Requeued`
(`LifecycleEvent::is_terminal`). Register hooks with `with_hook` /
`hooks().register(..)` on the in-memory pools, `DurableOptions::with_hooks`
on the durable pool, or `builders::build_pools_from_scheduler_config_with_hooks`.
Hooks run on the bus's own thread, in emission order; emission never blocks a
pool (a full queue drops the event and counts it in
`HookBus::dropped_events`), and a pool with no hook pays one atomic load per
transition. `HookBus::flush` waits for delivery. The WASM pool (`prometheus-parking-lot-wasm`)
emits the same events (cr-53) through `WorkerPool::with_hook` / `hooks()`; it has
no threads, so its hooks run inline on the emitting call, and `worker_id` is
always 0. Durable dispatch does not apply on the web target.

## Iggy events and notifications (feature `iggy`)

`infra::events` re-exports `prometheus-parking-lot-iggy` (cr-32). Iggy carries
events and notifications only; results stay in the mailbox storage and the work
queue stays in Postgres or redb.

| Item | What it does |
| --- | --- |
| `IggyOptions::new(connection_string)` | Stream `parking-lot`, topics `lifecycle` and `mailbox.ready`, 4 partitions, a 1024-event queue, a 2 s attempt timeout, `RetryPolicy::default()`, a 4096-id dedup window. `Debug` hides the password. |
| `IggyHook::start(&options)` | A `Hook`: register it on a pool's `HookBus`. Publishes every `LifecycleEvent` to `lifecycle`, partitioned by `options.tenant`, else by pool. |
| `IggyNotifier::start(&options)` | A `Notifier<T>`: wrap in `BackgroundNotifier::new(notifier, TokioHandleSpawner::current(), TokioSleeper)` and pass to `StorageMailbox::new(storage, ..)`. Publishes `mailbox.ready` (mailbox key, task id, status, `result_ref`) after the result is stored, partitioned by tenant. |
| `ids::message_id`, `ids::mailbox_ready_id` | UUIDv5 message ids (as `u128`) from the idempotency key, or from the mailbox address and task id when there is none. |
| `Publisher`, `PublisherStats` (`hook.stats()`, `notifier.stats()`) | `enqueued`, `published`, `dropped` (queue full), `deduplicated`, `retried`, `failed`. `flush(timeout)` waits for the queue to drain. |
| `IggyStreamSink::start(&options)` | Publishes task output streams to `stream.chunks` (cr-33). `writer(key)` (key = the idempotency key) gives a `StreamWriter`: `send(text)` returns the chunk's sequence number (from 1), `finish(StreamOutcome)` writes the end marker. `writer_from(key, next_seq)` continues after an executor restart. All messages of a stream go to one partition (`partition_for(key, partitions)`). |
| `IggyStreamReader::connect(&options)` | `open(key)` or `resume(key, ResumePoint { after_seq, offset })` returns a `StreamCursor`; `next().await` yields `StreamItem::Chunk` / `StreamItem::End` in order, waiting for live chunks, then `None`. `item.resume_point()` is what a client stores. |

Stream replay: the sequence number is the cursor, the offset is only a hint to
avoid re-reading the partition. The cursor delivers `last + 1` only: a lower
sequence number is a duplicate (retried or re-published chunk) and is skipped,
a higher one is `StreamError::Gap`. A chunk is never dropped on a full queue:
`send` waits for room, slowing the executor to the publisher's pace, and a
chunk abandoned after its retries poisons the writer (`StreamError::Poisoned`)
so the stream never continues past a hole. Wrap `next()` in a timeout: a
poisoned stream never gets an end marker.

Publishing is off the dispatch path: `Hook::on_event` only serializes and does
a non-blocking enqueue. While the server is slow or down the queue (default
1024) fills and further events are dropped and counted; each message gets
`RetryPolicy` attempts with a per-attempt timeout. Iggy 0.9 deduplicates
retried requests per client, not by message id, so the publisher skips an id it
published recently and a consumer that must be exactly-once across publisher
restarts de-duplicates on the message id. Use an IPv4 literal such as
`127.0.0.1` in the connection string when `localhost` resolves to `::1`.

## Scheduling internals (core crate only)

`prometheus_parking_lot_core::dispatch` (`Dispatcher`, `Admission`,
`Dispatched`, `Grant`, `Ticket`, `Parkable`, `ParkedQueue`, `PriorityQueue`,
`TaskQueueAdapter`, `SkipAheadStats`, `DEFAULT_MAX_HEAD_SKIPS`,
`ADAPTER_LOOKAHEAD`) and `prometheus_parking_lot_core::semaphore`
(`UnitSemaphore`, `Permit`, `UnitVector`, `KIND_COUNT`) are what every pool
admits through. The facade does not re-export them.

Multi-resource admission (cr-23): `Ticket::demand` is a `UnitVector` (total
units plus units per `ResourceKind`; `Ticket::units()` is the total).
`UnitSemaphore::with_resource_limit(kind, limit)` limits a kind;
`try_acquire_vector` claims a demand in every dimension at once or not at
all (`try_acquire(units)` is the single-dimension form);
`available()` returns the free `UnitVector` and `exceeded_limit` the first
dimension a demand can never fit. `ParkedQueue::fitting` takes the free
`UnitVector` instead of a unit count. `Dispatcher::check_capacity`,
`units_in_use_of`, `resource_limit` and `set_resource_limit` expose the same
per kind. Pools: `PoolConfig::resource_limits` and
`WorkerPoolConfig::resource_limits` (a map from kind to units, empty by
default), `ResourcePool::with_resource_limit` (before the first submit) and
`active_units_of`, `DurableOptions::with_resource_limit`, native
`WorkerPool::active_units_of`.

Admission order (cr-20): parked tasks start in priority, then submission,
order. When the head does not fit the free units, the first parked task that
does starts in its place (skip-ahead); after `max_head_skips` such skips
(default 8, `0` for strict head-of-line order) the dispatcher reserves the
capacity for the head and starts nothing else until it has started.
`ResourcePool::with_max_head_skips` and `skip_ahead_stats`,
`WorkerPoolConfig::max_head_skips` and the native `WorkerPool::skip_ahead_stats`
expose it; `DurableResourcePool` dispatches strictly head-of-line.
