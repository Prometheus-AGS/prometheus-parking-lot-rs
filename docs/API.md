# API overview

A map of the public API, including implemented 0.4 changes awaiting final verification. The rustdoc is the reference;
build it with `cargo doc --workspace --no-deps --all-features --open`. Compiled
examples are in the [README](../README.md). Design decisions are in
[`adr/`](adr/).

Most applications depend only on the facade crate `prometheus_parking_lot`.
Paths below are facade paths unless a crate is named.

## Pools

On `wasm32-unknown-unknown`, depend on the facade with `default-features = false`:
the default `tokio-runtime` feature needs tokio's multi-thread runtime, which does
not build for that target. The wasm `WorkerPool` is re-exported by target, not by
feature.

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

`ResourcePool`: `new`, `with_name` (default `resource_pool`; config-built
pools take their key in `pools`), `name`, `with_audit`, `with_clock`, `with_cancellation_sink`,
`with_cancellation_config`, `submit`, `prune_expired`, `active_units`,
`running_tasks`, `queued_tasks`, `shutdown` (refuse new work only),
`shutdown_with(policy)` (async; returns a `ShutdownReport`, cr-29).

`ResourcePool::prune_expired().await` retains its public
`Result<usize, SchedulerError>` removal count (oa-10, #57). Each removed task
uses the normal per-task expiry path outside the dispatcher lock: `Expired`
terminal event, one configured mailbox delivery attempt of `TaskStatus::Expired`
with no result payload, individual expiry audit, and once-only accepted-work
settlement. It cannot execute or be returned by a repeated prune. Equality at
a deadline is live; tasks without deadlines remain queued. Active permits
are unchanged, and live work unblocked by removal can start in the following
drain. This path invokes no cancellation sink.

Expiry counts as a failed task and a finished ledger outcome. Settlement
precedes mailbox/audit callbacks as for head expiry; a shutdown report is not
an acknowledgement that storage succeeded. Mailbox errors, callback failures
and bounded hook delivery retain their existing limits. A conforming backend
removes nothing on a pruning error, and no per-task expiry reporting runs then.

`ResourcePool::new` requires `Q: TaskQueue<P>` and includes the queue's
existing reported depth in shutdown accounting (oa-09). Construction does not
spawn work. After builder configuration, the first submission or
`shutdown_with` installs the release hook and drains pre-filled entries;
other draining operations can also observe the head.

A stored head exceeding the absolute total or per-kind limit is removed once
without execution, emits `Rejected` with `CapacityExceeded` identifying the
dimension, and gets one configured mailbox delivery attempt of
`TaskStatus::Dropped(capacity_error.to_string())`, with no result payload.
It counts in `failed_tasks` and as a finished ledger outcome, then settles;
it does not invoke the cancellation sink. Pre-filled tasks get no fabricated
`Submitted`/`Queued` event from this pool. Valid heads waiting only for units
held by running tasks remain parked. Head expiry and valid completion use
their usual per-task accounting, including when shutdown is the first pool
operation. An impossible head is capacity-rejected before its expiry is checked.

FIFO follows the stored tickets and configured dispatch policy. The pool does
not recover a global sequence counter across an arbitrary imported sequence
epoch and new submissions. The initial ledger count relies on the backend's
reported depth; external additions after ownership transfer are outside this
accounting. Mailbox errors, callback panics and bounded hook delivery retain
their existing limits. This in-memory path adds no persistent dead-letter store.

`Spawn` may legally drop a supplied future without polling it. For an accepted
`ResourcePool` run discarded this way (oa-08), the executor is never called:
the pool emits one `Failed` lifecycle event with no `Started`, and attempts one
mailbox delivery for a configured key, with no result payload and status
`TaskStatus::Failed(TaskError::Cancelled { reason })`. The reason is exactly
`spawner discarded resource pool task before its first poll`.

The pool returns the permit and launches admitted parked work before mailbox
or audit callbacks, increments `failed_tasks`, and settles shutdown accounting
when the discard action exits. A discard during shutdown counts in
`ShutdownReport::completed` (which includes finished failures); one settled
before shutdown began is outside that report's interval. It does not increment
`cancelled`, emit the shutdown `Cancelled` event, invoke the asynchronous
`CancellationSink`, or requeue the payload. First-polled runs retain the normal
executor panic, timeout and shutdown cancellation behavior.

Discard settlement is synchronous and does not supply an unavailable runtime.
A spawner that retains a future without polling or dropping it can still hold
its work unsettled. Collaborators must return for settlement to finish. The
discard path contains mailbox/audit callback unwinding and settles the ledger,
but does not guarantee a stored result when the mailbox errors or panics before
storage. Panicking clocks/queues can interrupt completion, blocking callbacks
can delay it, process aborts do not run drop guards, and hook delivery retains
the bus's bounded-queue semantics. `ResourcePool` remains an in-memory pool.

`DurableResourcePool` (crash-durable mode on a `LeaseQueue` and a
`ResultMailbox`, cr-49; top-level `DurableResourcePool`, `DurableOptions`):
`start(limits, queue, mailbox, executor, spawner, options)`, `submit` (returns
once the task is persisted), `queue`, `mailbox`, `options`, `active_units`,
`running_tasks`, `completed_tasks`, `failed_tasks`, `shutdown` (stop leasing
only), `shutdown_with(policy)` (async; hands cancelled tasks back to the queue
and returns a `ShutdownReport`, cr-29).
`DurableOptions::new(lease_for)` with `with_heartbeat_interval`,
`with_poll_interval`, `with_result_ttl`, `with_name` (logs and events only;
results are keyed by `LeaseQueue::identity`, not the name), `with_resource_limit`,
`with_hooks`, `with_max_running` (running-task bound, default `max_units`).
`LeaseQueue::identity` (required): a stable name for the queue that qualifies
its entry ids.

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
| `BackendConfig` (`"type": "postgres"` / `"redb"` / `"pgmq"` / `"iggy"`), `PostgresBackendConfig`, `RedbBackendConfig`, `PgmqBackendConfig`, `IggyBackendConfig` (`options()` with `iggy`; topics `lifecycle_topic`, `mailbox_topic`, `stream_topic`) | `config::*` | always (validation requires `postgres` / `embedded` / `iggy`) |
| `ConfigError` (`Parse` names an unknown or missing field; `Invalid { field, reason }` with a dotted path) | `config::ConfigError` | always |
| `build_pools_from_scheduler_config(cfg, executor_factory, spawner)` → `BuiltScheduler { pools, durable_pools, postgres, pgmq, redb }`. Per-field features: `pools` always (within `tokio-runtime`); `durable_pools` needs `postgres` or `embedded`; `postgres` and `pgmq` need `postgres`; `redb` needs `embedded` | `builders::*` | `tokio-runtime` |
| `ConfiguredDurablePool` (`Postgres` / `Redb` variants; `submit`, `consume`, `shutdown`) | `builders::*` | `tokio-runtime` and (`postgres` or `embedded`); the `Postgres` variant needs `postgres`, the `Redb` variant `embedded` |
| `ConfiguredPool { pool, mailbox }`, `SharedInMemoryMailbox` (`take`) | `builders::*` | `tokio-runtime` |
| `PostgresBackend { queue, mailbox, listener, lease_for }`, `PgmqBackend`, `RedbBackend { store, queue, mailbox, lease_for }` | `builders::*` | `tokio-runtime` with `postgres` (`PostgresBackend`, `PgmqBackend`) or `embedded` (`RedbBackend`) |
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
| `SchedulerError` | Admission (`QueueFull`, `CapacityExceeded { pool, kind, requested, available }` — `kind` is the limited `ResourceKind` that did not fit, `None` for the total; the variant is `#[non_exhaustive]`, build it with `SchedulerError::capacity_exceeded` — `DeadlineExpired`, `PoolShutdown`), `InvalidConfig`, `Backend`, `Timeout` (a wait for a result), `ResultNotFound`, `LeaseLost`, `Internal`, and `Task(TaskError)` |
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

Synchronous pruning returns removed tasks rather than a count:
`TaskQueue<P>::prune_expired(&mut self, now_ms)` returns
`Result<Vec<ScheduledTask<P>>, SchedulerError>`. Each removed task retains its
full original metadata and payload, exactly once, without requiring `P: Clone`.
Expiry is strict `now_ms > deadline_ms`; equality and missing deadlines stay
live. Survivors retain their ordering metadata, removed-item ordering is
unspecified, and `Err` must leave every task queued. Custom backends must
implement this ownership-returning contract rather than discarding removed
metadata. The synchronous conformance helper is
`prometheus_parking_lot_core::conformance::task_queue::prune_returns_all_removed_tasks`
with feature `conformance`.

Core-only `dispatch::ParkedQueue<T>::prune_expired` analogously returns
`Result<Vec<T>, SchedulerError>`. `TaskQueueAdapter` completes fallible backend
pruning before mutating its in-memory front. `Dispatcher::prune_expired` keeps
its `(usize, Dispatched<T>)` result and places full removed tasks in `expired`;
callers must report those outcomes outside the dispatch lock. Custom queue
implementations and direct consumers must preserve that reporting obligation.

The async contracts live in `core::backend`; `DurableResourcePool` runs on them.
`StorageMailbox::new(storage, notifier)` takes a `DetachedNotifier`:
`NoopNotifier`, or `BackgroundNotifier::new(notifier, spawner, sleeper)`, which
runs any `Notifier` off the delivery path with bounded retries (`with_policy(RetryPolicy)`,
`stats()` returning `NotifyStats` / `NotifyStatsSnapshot`; cr-34). All of them
are in `core::backend` and at `core::*`.
The backend conformance suite is `prometheus_parking_lot_core::conformance`
behind that crate's `conformance` feature.

### Address-scoped result idempotency (0.4, oa-06)

A result's first-write-wins identity is `(storage namespace, complete MailboxKey,
caller idempotency key)`. Tenant, user and session all participate; `None` and
an empty optional identifier are distinct. Two addresses may reuse the same
caller key independently. Same-address retries retain the original fence
through consume/reopen until its original expiry; no-TTL fences remain
permanent. Expiry at one address never releases another address's fence.
Unkeyed delivery is unchanged.

### Authoritative duplicate status and final schema 3 (0.4, oa-07)

`DeliveryOutcome::Stored` remains a unit variant;
`DeliveryOutcome::Duplicate { status: TaskStatus }` returns the original full
status, including the original `TaskError`, atomically with the duplicate
choice. Status survives result consumption for the original fence lifetime.
A retry neither replaces the result/status nor extends TTL. Missing or
undecodable authoritative metadata is an error, never a successful duplicate
using the retry's status. No destructive consume is needed to learn status.
The enum loses `Copy` and `Hash`; match its payload, use `Duplicate { .. }`
when intentionally ignoring metadata, or clone. Mailbox method signatures are
unchanged. `StorageMailbox` notifies only `Stored` and passes metadata through.
The FFI's existing `DeliveryResult::Duplicate` label intentionally omits it.

Core mapping types are `prometheus_parking_lot_core::LegacyIdempotencyOwner`
(`idempotency_key: String`, `key: MailboxKey`) and `LegacyDeliveryStatus`
(the same fields plus `status: TaskStatus`), each with `new(...)`, serde and
unknown-field rejection. Owner evidence makes no status claim. These are core
exports, not new facade `core::backend` exports. Supported entries are:

| Entry | Final upgrade behavior |
| --- | --- |
| `PgResultMailbox::<T>::migrate(&pool).await` | Creates/reopens protocol 3; converts legacy sources only if original owners/statuses are provable without supplied evidence. |
| `PgResultMailbox::<T>::migrate_with_idempotency_owners(&pool, &owners).await` | Owner-only evidence `&[(String, LegacyIdempotencyOwner)]`; supplies no missing status. Each outer string is the exact mailbox name. |
| `PgResultMailbox::<T>::migrate_with_idempotency_metadata(&pool, &owners, &statuses).await` | Atomic direct 0.3 -> 3 or intermediate 2 -> 3; status evidence is `&[(String, LegacyDeliveryStatus)]`. |
| `EmbeddedStore::open(path)` / `open_with_clock(path, clock)` | Creates/reopens schema 3; refuses schemas 1/2. |
| `EmbeddedStore::open_with_idempotency_metadata(path, &owners, &statuses)` / `open_with_clock_and_idempotency_metadata(path, clock, &owners, &statuses)` | Explicit atomic 1/2 -> 3 or schema-3 evidence replay, with core record slices (no outer namespace tuple). Owner-only entry variants remain but supply empty status evidence. |

Consumed 0.3 fences lack owner and status; intermediate schema-2 fences retain
owner but lack status. Resolve every active fence from a coherent retained
full-address/caller/exact-expiry generation or authoritative original evidence;
ambiguous assignments must select a retained candidate. Missing, duplicate,
unknown, expired, wrong-address or contradictory initial evidence refuses
atomically. Direct conversion cannot commit version 2 before missing-status
refusal. A consumed permanent fence cannot be recovered merely by waiting.

PG active `pl_result_mailbox_v3`/`pl_result_mailbox_keys_v3` and immutable
`pl_result_mailbox_metadata_v3` retain native expiry/status. PG rejects actual
v1/v2 mutation SQL at guarded retired names. Redb uses `mb_idem_v3` active
metadata and `mb_legacy_metadata_v3` proof, retaining entries, queue identity,
older fences and owner provenance; both old open protocols reject schema 3.
Replaying recorded evidence never recreates an expired fence or resets TTL.
Stop all old readers/writers, preserve an untouched backup and rehearse on a
copy. Rollback stops new writers and restores that backup; later writes need
separate reconciliation. Builders collect no missing metadata authority.

On duplicate delivery the durable pool chooses both its event and lease
resolution from original status: `Completed` -> completed/ack;
`Failed(error)` -> failed/dead-letter; `Expired` -> expired/dead-letter;
`Dropped(reason)` -> rejected with `Custom(reason)`/dead-letter. Delivery
precedes resolution. Generic mailbox `Queued`/`Running` remains supported;
a durable duplicate of either warns and requeues without fabricated terminal
truth, ack or dead-letter. A permanent nonterminal fence can retry indefinitely
until separate explicit remediation; finite fences keep their original TTL.
Local execution counters still describe attempts. Existing shutdown, oversize
and resolution retry policies remain intact.

See [ADR 0006](adr/0006-tenant-scoped-result-idempotency.md) for address scope
and [ADR 0007's final-schema runbook](adr/0007-authoritative-duplicate-status.md#operational-runbook-for-final-schema-3)
for exact mapping/backup/replay/rollback rules. PG17/PG18/redb migration and real
Failed-after-delivery process-kill fixtures, including consume-before-retry,
are authored; all final-boundary execution remains `NOT_EXERCISED`. This
API description authorizes no live upgrade.

## Hooks and audit

`core::hooks` (`Hook`, `HookBus`, `LifecycleEvent`, `TracingHook`,
`RecordingHook`) and `core::audit` (`AuditSink`, `AuditEvent`,
`InMemoryAuditSink`; `PostgresAuditSink` with `postgres`: `migrate(&pool)`,
`spawn(pool, capacity)`, `lost()`, `shutdown().await`; it writes through a
background task, so `record` never blocks and a full buffer drops and counts).

`ResourcePool`, the native `WorkerPool` and `DurableResourcePool` emit, per
task, `Submitted`, then `Queued` if it parks, `Started` when the executor is
about to run, and exactly one terminal event: `Completed`, `Failed`,
`Rejected`, `Cancelled`, `Expired` or (durable pool) `Requeued`
(`LifecycleEvent::is_terminal`); explicit pruning emits `Expired` for each
removed task, without `Started` (oa-10, #57).
`RejectionReason::CapacityExceeded { kind }` names the dimension
that did not fit (`None`: the pool's total). For `DurableResourcePool` the one-terminal rule holds per
run, not per task: a run that ends in `Requeued` is followed by the task's
next run (`Queued`/`Started`, then its own terminal event), here or in another
process. A durable `submit` whose enqueue fails with an unknown outcome emits
no terminal event. Register hooks with `with_hook` /
`hooks().register(..)` on the in-memory pools, `DurableOptions::with_hooks`
on the durable pool, or `builders::build_pools_from_scheduler_config_with_hooks`.
Hooks run on the bus's own thread, in emission order; emission never blocks a
pool (a full queue drops the event and counts it in
`HookBus::dropped_events`), and a pool with no hook pays one atomic load per
transition. `HookBus::flush` waits for delivery. The WASM pool (`prometheus-parking-lot-wasm`)
emits the same events (cr-53) through `WorkerPool::with_hook` / `hooks()`; it has
no threads, so its hooks run inline on the emitting call (`Queued` under the
dispatch lock: a hook must not call back into the pool; a hook panic aborts
the module), and `worker_id` is always 0. Durable dispatch does not apply on the web target.

## Iggy events and notifications (feature `iggy`)

`infra::events` re-exports `prometheus-parking-lot-iggy` (cr-32). Iggy carries
events and notifications only; results stay in the mailbox storage and the work
queue stays in Postgres or redb.

| Item | What it does |
| --- | --- |
| `IggyOptions::new(connection_string)` | Stream `parking-lot`, topics `lifecycle` and `mailbox.ready`, 4 partitions, a 1024-event queue, a 2 s attempt timeout, `RetryPolicy::default()`, a 4096-id dedup window. `Debug` hides the password. |
| `IggyHook::start(&options)` | A `Hook`: register it on a pool's `HookBus`. Publishes every `LifecycleEvent` to `lifecycle`, partitioned by `options.tenant`, else by pool. |
| `IggyNotifier::start(&options)` | A `Notifier<T>`: wrap in `BackgroundNotifier::new(notifier, TokioHandleSpawner::current(), TokioSleeper)` and pass to `StorageMailbox::new(storage, ..)`. Publishes `mailbox.ready` (mailbox key, task id, status, `result_ref`) after the result is stored, partitioned by tenant. |
| `ids::message_id`, `ids::mailbox_ready_id` | UUIDv5 message ids (as `u128`) derived from a stable key: the idempotency key of a `mailbox.ready` notification, or a stream key and sequence number for a chunk. A keyless `mailbox.ready` and every lifecycle event get an id from a per-instance nonce and a sequence number instead: unique, not reproducible across processes. |
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
`RetryPolicy` attempts with a per-attempt timeout. The Iggy server (0.9) deduplicates
retried requests per client, not by message id, so the publisher skips a
`mailbox.ready` or stream-chunk id it published recently (lifecycle events
bypass that window) and a consumer that must be exactly-once across publisher
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
`units_in_use_of`, `resource_limit` and `set_resource_limit` (before the
first submission only) expose the same per kind. A cost whose per-kind units
sum past `u32::MAX` exceeds every total. Pools: `PoolConfig::resource_limits` and
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
expose it; `DurableResourcePool` dispatches strictly head-of-line. With
`max_queue_depth` items parked, a submission that would start at once (a
higher priority than the head that fits, or a permitted skip) is admitted;
only one that would wait gets `QueueFull`. That at-depth skip needs
`ParkedQueue::supports_skip_ahead`.

Returning permits: a `Grant`'s permit goes back through
`Dispatcher::complete`, which drains. A permit dropped any other way calls the
hook installed with `Dispatcher::install_release_hook`, which should schedule
a `Dispatcher::poke`; without one, the owner must poke. A dispatcher whose
semaphore allows 0 permits refuses submissions with `InvalidConfig`.
