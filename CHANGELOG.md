# Changelog

## [0.4.0] - 2026-10-24

### prometheus-parking-lot-core

- feat: implement operational assurance for 0.4.0 (16db521829d83ba140bb5ce2ea8f8d3252acf35d)

### prometheus-parking-lot-runtime-tokio

- feat: implement operational assurance for 0.4.0 (16db521829d83ba140bb5ce2ea8f8d3252acf35d)

### prometheus-parking-lot-pg

- feat: implement operational assurance for 0.4.0 (16db521829d83ba140bb5ce2ea8f8d3252acf35d)

### prometheus-parking-lot-embedded

- feat: implement operational assurance for 0.4.0 (16db521829d83ba140bb5ce2ea8f8d3252acf35d)

### prometheus-parking-lot-iggy

- feat: implement operational assurance for 0.4.0 (16db521829d83ba140bb5ce2ea8f8d3252acf35d)

### prometheus-parking-lot-wasm

- feat: implement operational assurance for 0.4.0 (16db521829d83ba140bb5ce2ea8f8d3252acf35d)

### prometheus_parking_lot

- feat: implement operational assurance for 0.4.0 (16db521829d83ba140bb5ce2ea8f8d3252acf35d)

All notable changes to `prometheus_parking_lot` are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Durable stored outcome (oa-07, #78).** A duplicate returns its original full
  status/error even after result consumption. After a Failed delivery and
  process crash, a locally successful retry emits the original Failed and
  dead-letters rather than emitting Completed and acking. Delivery still
  precedes lease resolution and only the original result is consumable.
  Generic Queued/Running metadata remains supported; the durable consumer warns
  and requeues rather than fabricating terminal truth. A permanent nonterminal
  fence can retry indefinitely pending explicit remediation. Real PG17/18 and
  redb crash/restart/consumed-result fixtures are authored; execution remains
  NOT_EXERCISED.

- **Per-task explicit expiry (oa-10, #57).** `ResourcePool::prune_expired`
  retains its public removal count and now reports each removed task through
  the normal expiry path: `Expired` terminal event, one configured mailbox
  delivery attempt of `TaskStatus::Expired`, per-task audit and once-only
  ledger settlement. Removed work cannot execute, and live survivors keep
  their order. Equality at a deadline remains live. Aggregate accounting and
  the batch-only audit are replaced by per-task handling; callback/storage
  failure limits remain unchanged. Final-boundary execution is unverified.

- **Address-scoped result idempotency (oa-06, #80).** Results now deduplicate
  by the complete mailbox address plus caller key within the storage namespace.
  Different tenants, users or sessions can independently reuse one caller key;
  same-address retries retain the first result through consume/reopen until
  its original TTL, and no-TTL fences remain permanent. The memory reference,
  Postgres and redb implementations share these semantics. Final-boundary
  execution remains unverified.

- **Pre-filled ResourcePool admission (oa-09, #82).** A stored head exceeding
  the current total or per-kind capacity is removed once without execution,
  emits `Rejected(CapacityExceeded)`, and gets one configured mailbox delivery
  attempt of `TaskStatus::Dropped(capacity_error.to_string())` with no result
  payload. Valid followers proceed; valid heads waiting for temporarily held
  units remain parked. Pre-filled depth now enters shutdown accounting at
  construction and is first drained after builder configuration on submission
  or shutdown, so rejection, head expiry and completion settle counted work.
  FIFO retains the stored-ticket policy; no cross-epoch global sequence
  recovery or persistent dead-letter store is added.
- **Discarded ResourcePool spawns (oa-08, #81).** An accepted run whose
  spawner drops its future before first poll now emits `Failed` and attempts
  one configured mailbox delivery of `TaskStatus::Failed(TaskError::Cancelled)`
  with reason `spawner discarded resource pool task before its first poll`
  and no result payload. Its permit is returned and shutdown accounting is
  settled; the executor is never called. This finished failure does not invoke
  the async cancellation sink or increment `ShutdownReport::cancelled`.
  Retained futures, unavailable execution runtimes and failed mailbox storage
  retain their existing progress/delivery limits; this adds no crash durability.

### Changed (breaking)

- **Pruned-task ownership (oa-10).** Synchronous `TaskQueue<P>::prune_expired`
  now returns `Result<Vec<ScheduledTask<P>>, SchedulerError>`; core
  `ParkedQueue<T>::prune_expired` returns `Result<Vec<T>, SchedulerError>`.
  Custom implementations must return every removed task with original
  metadata/payload, without requiring Clone, and leave tasks queued on error.
  Adapter pruning performs the fallible backend step before changing its
  front. Direct dispatcher consumers retain the count plus full removed
  items in `Dispatched.expired` for terminal reporting outside the lock.
- **Duplicate metadata (oa-07).** `DeliveryOutcome::Duplicate` becomes
  `Duplicate { status: TaskStatus }`; the enum loses Copy/Hash. Custom mailbox
  implementations must retain original status atomically with the fence beyond
  consume, and never substitute retry status or extend original TTL. Stored
  remains a unit variant; existing mailbox method signatures are unchanged.
- **Persistent result-mailbox upgrade (oa-06/oa-07).** Final 0.4 uses PG
  protocol 3 and redb schema 3. Both old 0.3 and intermediate schema-2 writers
  are rejected. Core `LegacyIdempotencyOwner` and `LegacyDeliveryStatus`
  provide independent owner/status evidence through explicit metadata APIs.
  Direct 0.3 -> 3 and intermediate 2 -> 3 conversion refuse atomically if
  original active metadata cannot be proved; no owner, status or TTL is
  invented. Results/identity/order and immutable provenance are preserved;
  evidence replay cannot resurrect expired fences. Ordinary embedded open
  refuses 1/2. Rollback requires an untouched pre-upgrade backup with new
  writers stopped. See ADRs 0006/0007; implementation is not live-upgrade
  authorization or execution evidence.
- **Core dispatcher result (oa-09).** `Dispatched<T>` adds
  `rejected: Vec<(T, SchedulerError)>`; callers must report every returned
  rejection outside the dispatch lock. Struct literals/destructuring must
  account for the field, and rejection-only batches are nonempty.
- **ResourcePool constructor bound (oa-09).** `ResourcePool::new` now requires
  `Q: TaskQueue<P>` to count already queued work before observing it.

## [0.3.0] - 2026-10-24

0.3.0 is source-breaking. The design records are `docs/adr/0001-workspace-split.md`,
`docs/adr/0002-core-contracts.md` and `docs/adr/0003-backend-contracts.md`. Each
entry names the OpenSpec change that made it.

### Changed (breaking)

- **`LeaseQueue::identity` (0.3.0 gate review).** `LeaseQueue` has a new
  required method, `fn identity(&self) -> &str`: a stable name for the queue,
  the same in every process and across restarts, that qualifies its entry ids.
  The built-in backends return `in-memory:<process nonce>:<instance>`,
  `pg:pl_lease_queue:<queue name>`, `pgmq:<queue name>` and `redb:<store id>`
  (a random id generated when the file is first opened and stored in it).
  Custom queues and queue wrappers must implement it (wrappers forward it).
  The backend conformance suite checks it
  (`identity_is_stable_and_distinct`).
- **Durable result keys.** A `DurableResourcePool` result delivered
  without a task idempotency key is keyed `"<queue identity>:entry:<entry
  id>"` instead of `"<pool name>:entry:<entry id>"`. `DurableOptions::with_name`
  now names the pool in logs, errors and events only. Pre-release users:
  drain durable queues before upgrading, or a task in flight across the
  upgrade can deliver a second result under the new key.
- **Durable lifecycle events.** `DurableResourcePool` emits one terminal
  event per run, not per task: a run that ends in `Requeued` is followed by
  the task's next run. An enqueue that fails with an unknown outcome (a
  backend error) no longer emits `Rejected`, because the task may have been
  stored and may run; definite refusals (`QueueFull`, `CapacityExceeded`,
  `DeadlineExpired`, `PoolShutdown`, `InvalidConfig`) still do.
- **Durable oversize tasks.** A leased task too large for this pool is
  handed back (`nack`, requeued after one lease duration) instead of being
  delivered as `Dropped` and dead-lettered, so a consumer with more capacity
  can run it. It emits `Requeued`, not `Rejected`.
- **Durable oversize bound.** The hand-back above is bounded by
  `DurableOptions::with_oversize_attempt_limit` (default 5). A pool that
  leases such a task at that attempt or later delivers `TaskStatus::Dropped`
  to its mailbox key, emits `Rejected` (`CapacityExceeded`) and dead-letters
  it, so the client gets a terminal outcome instead of the task cycling
  forever. A consumer that fits the task still runs it at any attempt.
- **Pre-write enqueue failures are refusals.** New
  `SchedulerError::NotStored` (constructor `SchedulerError::not_stored`) for a
  backend failure that certainly stored nothing, and
  `RejectionReason::NotStored`. The Postgres, pgmq and redb queues return it
  from `enqueue` for failures before the commit (serialisation, `begin`, the
  statements of the uncommitted transaction); `DurableResourcePool::submit`
  emits `Rejected` for it. `SchedulerError::Backend` from `enqueue` still
  means "outcome unknown" and still emits no terminal event.
- **Executor panic racing shutdown.** A durable task whose
  executor panics while `shutdown_with` fires its cancel token now fails
  (`Failed`, delivered and dead-lettered) instead of being handed back; only
  a panic raised while dropping the executor future after the cancel branch
  won is treated as a cancellation.
- **`ResourcePool::with_name` docs.** Its `# Panics` section
  now says what it does: it panics after the first `submit`, even once the
  pool is idle again, like `with_resource_limit`.

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
  `#[non_exhaustive]`. Since cr-23 a pool can also limit each kind (below).
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
  `.mailbox` are `"in_memory"` or `{ "backend": name }`; since durable
  dispatch (cr-49, under Added) a pool naming a backend for both runs durably
  on it. The ignored `runtime` field and
  `RuntimeConfig`, `QueueBackendConfig` and `MailboxBackendConfig` are
  removed. `validate` and `from_json_str` return `ConfigError` (`Parse` names
  an unknown or missing field; `Invalid` names the field by dotted path, e.g.
  `pools.inference.max_units`) instead of `String`; `build_pools` returns a
  `BTreeMap`. `WorkerPoolConfig::thread_stack_size` exists on every target
  (ignored on WASM), and `default_timeout_ms` is documented as the execution
  budget (and the native default result TTL), not a `retrieve` timeout.
- **Builder spawner bound (cr-49).** `build_pools_from_scheduler_config` now
  requires `S: Spawn + Clone + Send + Sync + 'static` (it was `S: Clone`),
  because a durable pool starts its dispatcher while it is built.
  `TokioSpawner` satisfies it. `build_pools` rejects a configuration whose
  pools name a durable backend; build those with
  `build_pools_from_scheduler_config`.

- **Shutdown and cancellation capture (cr-29).** `CancelledTask::payload` is
  now `Option<P>`: a task that never started always carries its payload, a
  running task cancelled by `ResourcePool` carries it while a sink is
  registered (restored from a JSON snapshot), and one cancelled by the native
  `WorkerPool` carries `None`. `LoggingCancellationSink` no longer requires
  `P: Debug + Sync`. `WorkerPool::shutdown()` is now
  `shutdown_with(ShutdownPolicy::Drain { deadline: 0 })`: tasks still waiting
  are handed to the cancellation sink (their result slot still gets
  `TaskError::Cancelled`), and it waits about 2 s in total (0.2 waited up to
  2 s per worker) for workers to exit before detaching them. An admitted task
  a worker receives after `shutdown` began now runs instead of being failed
  as cancelled; once the cancel phase of `shutdown_with` has begun it is
  captured to the sink. `ShutdownHandle::shutdown_with` is documented as what
  it always did, firing the token; the policy is applied by the pools' own
  `shutdown_with`. `ResourcePool::submit` and `ResourcePool::shutdown` now
  use `SeqCst` on the shutdown flag (no behaviour change). The runtime crate
  depends on `serde_json` (payload snapshots).

- **Hook delivery (cr-31).** `HookBus` no longer calls hooks inline: it is a
  cheap-to-clone handle whose `register` takes `&self`, and events reach the
  hooks on the bus's own thread (`pl-hooks`), in emission order, after
  `emit` returns. Call `HookBus::flush(timeout)` to wait for delivery.
  `LifecycleEvent` gains `Expired` and `Requeued`, and `RejectionReason`
  gains `CapacityExceeded` (both enums are `#[non_exhaustive]`).

- **Vector admission internals (cr-23).** In `prometheus-parking-lot-core`,
  `dispatch::Ticket`'s `units: u32` field is replaced by `demand: UnitVector`
  (`Ticket::units()` returns the total); `ParkedQueue::fitting` takes the free
  capacity as `&UnitVector` instead of a `u32`; `semaphore::Permit` holds a
  `UnitVector` (`units()` still returns the total). `PoolConfig` and
  `WorkerPoolConfig` gain a public `resource_limits` field, so struct
  literals of them need `resource_limits: BTreeMap::new()` (serialized
  configurations are unaffected: the field is optional).
- **Skip-ahead is on by default (cr-20).** The in-memory pools no longer
  stop at a parked head that does not fit: a smaller parked task that fits
  starts in its place, at most 8 times per head (see Added). Set
  `max_head_skips` to `0` (`PoolConfig`, `WorkerPoolConfig`,
  `ResourcePool::with_max_head_skips`, `Dispatcher::with_max_head_skips`) for
  the previous strict head-of-line order. `PoolConfig` and `WorkerPoolConfig`
  gain the public field `max_head_skips` (optional in JSON, default 8), so a
  struct literal of either must now set it.

- **`CapacityExceeded` names the dimension (cr-23).**
  `SchedulerError::CapacityExceeded` gains `kind: Option<ResourceKind>`: the
  per-kind limit the task exceeded, or `None` for the pool's total.
  `available` is that dimension's limit. The message names the kind
  (`need 30 gpu_vram units, pool 'inference' has 24`). The variant is now
  `#[non_exhaustive]`: match it with `..` and build it with the new
  `SchedulerError::capacity_exceeded(pool, kind, requested, available)`. A
  durable pool's `TaskStatus::Dropped` reason for a task that can never fit is
  now this error's message.
- **Configured pool names in errors and events (cr-37, cr-23).** New
  `ResourcePool::with_name` and `ResourcePool::name` (default
  `resource_pool`); `build_pools` and `build_pools_from_scheduler_config` name
  each in-memory pool after its key in `pools`, as they already did for durable
  pools (`DurableOptions::with_name`). `CapacityExceeded`, `QueueFull` and
  `PoolShutdown` from such a pool, its lifecycle events and its cancellation
  log now carry that name instead of `resource_pool`. In the core crate,
  `Dispatcher::pool_name` and `Dispatcher::set_pool_name` are new.
- **`RejectionReason::CapacityExceeded` names the dimension (cr-23).** It is
  now `CapacityExceeded { kind: Option<ResourceKind> }`: the limited kind the
  task exceeded, or `None` for the pool's total, the same dimension as
  `SchedulerError::CapacityExceeded`. Every pool (`ResourcePool`, the native
  and WASM `WorkerPool`s, `DurableResourcePool`) fills it in its `Rejected`
  event. Match it with `CapacityExceeded { .. }`.
- **Resource limits are set before the first submission (core).**
  `Dispatcher::set_resource_limit` now returns `InvalidConfig` once anything
  has been submitted (it used to check only for outstanding permits, so an
  item already parked could end up over the new limit and block the queue
  for good). `ResourcePool::with_resource_limit` and `with_name` panic after
  the first `submit`, as documented, even once the pool is idle again: the
  pool's state is shared from then on.
- **A zero-permit semaphore is refused (core).** `Dispatcher::submit` returns
  `SchedulerError::InvalidConfig` when its semaphore admits no permit
  (`UnitSemaphore::with_max_permits(_, 0)`) instead of parking every item
  forever. The pools already reject `worker_count == 0` in their
  configuration.

### Fixed

- **Postgres pre-release re-review (postgres-engineer).** `COMMENT ON` in
  migrations 004, 005 and 007 took `SHARE UPDATE EXCLUSIVE` on every re-run, so
  a manual `VACUUM` or `pg_repack` could fail start-up under the 5 s
  `lock_timeout`; each comment is now set only when it differs. A heartbeat
  `extend` already waiting on the row lock behind `nack(Requeue { delay > 0 })`
  no longer re-arms the nacked pgmq token (the fence reads the delay mark
  through the volatile `pl_pgmq_is_delayed`, migration 006). A pgmq `lease`
  whose read of a lower lane fails now returns the tasks higher lanes already
  leased instead of discarding them. One audit event the database rejects no
  longer loses the rest of its batch.

### Added

- **`PostgresAuditSink` writes events (audit finding).** It was an empty stub
  that silently dropped every event. `PostgresAuditSink::spawn(pool, capacity)`
  now queues `record`ed events on a bounded channel and a background task
  inserts them in batches into `pl_audit_events` (migration
  `007_audit_events.sql`, applied by `PostgresAuditSink::migrate`). It never
  blocks `record`; events dropped on a full buffer or that the database rejects
  (a batch it rejects is retried event by event, so one bad event, such as a NUL
  byte, loses only itself) are counted (`lost()`) and logged, and `shutdown().await` drains the queue.
  `payload` is `TEXT` (the old, never-applied DDL said `JSONB`). Needs a Tokio
  runtime.
- **Iggy topic retention.** `IggyOptions::with_retention` and
  `"retention_secs"` in the `iggy` backend section set the expiry of topics the
  crate creates. The default is unchanged (never expire), so operators must size
  the server's disk or set it.

- **`RedbLeaseQueue::with_max_attempts` and `max_attempts` in the `redb`
  backend section.** A task is delivered exactly that many times, then the next
  lease dead-letters it with the reason `max attempts (N) exhausted`, the same
  rule as `PgLeaseQueue::with_max_attempts`. Without it a task that kills a
  desktop app is redelivered, and kills it again, on every launch. The field is
  optional and must be greater than 0 (`examples/configs/redb.json` sets 5).

- **Notifier composition in the facade (cr-34, cr-32, cr-47).**
  `BackgroundNotifier`, `DetachedNotifier`, `RetryPolicy`, `Sleeper`,
  `NotifyStats` and `NotifyStatsSnapshot` are re-exported at
  `prometheus_parking_lot::core` and `core::backend`, beside `StorageMailbox`.
  A facade-only application can now wrap `infra::events::IggyNotifier` in a
  `StorageMailbox` without depending on `prometheus-parking-lot-core`; the
  `iggy_events` example imports through the facade only.
- **`stream_topic` in the Iggy configuration (cr-33, cr-37).**
  `IggyBackendConfig` gains `stream_topic` (default `stream.chunks`, the
  `IggyOptions` default; must not be empty), and `options()` passes it to
  `IggyOptions::with_stream_topic`, so a configuration file can name the topic
  `IggyStreamSink` and `IggyStreamReader` use. The field is optional in JSON;
  a struct literal of `IggyBackendConfig` must now set it.
- **Scenario guides and examples (cr-47).** The docs site gains guides for a
  GPU inference pool, a Tauri local LLM, a multi-tenant Postgres service,
  disconnected clients, shutdown and requeue, Iggy events, streaming replay,
  configuration, upgrading from 0.2 and the API reference (docs.rs from the
  0.3.0 release). Each is backed by a new example: `gpu_inference_pool`,
  `disconnected_client`, `shutdown_requeue` (no server), `postgres_tenants`
  (PostgreSQL), `iggy_events` and `iggy_stream_replay` (Iggy 0.9). Every Rust
  block on the site must be a verbatim excerpt of an example
  (`.github/scripts/check-doc-snippets.py`), and the `Docs examples` workflow
  builds every example and runs each one, the Postgres and Iggy ones against
  containers.
- **Benchmark suites and CI job (cr-40).** Four criterion suites replace
  `queue_bench` and `skip_ahead_bench`: `components` (semaphore, dispatcher,
  queue, mailbox, hooks), `throughput` (tasks per second through
  `ResourcePool`, the native `WorkerPool` and `DurableResourcePool` on the
  in-memory backends and on redb), `latency` (submit-to-start and
  submit-to-result p50/p95/p99 under light and saturated load, with the
  spread across rounds) and `utilization` (the cr-20 simulation extended with a
  cost-vector workload). Each suite writes machine and toolchain metadata with
  its numbers (`target/bench-report/`). CI job `Benchmarks` runs them in a
  reduced configuration on every push and pull request, stores the results as
  the `bench-results-<sha>` artifact, and compares pull requests with the last
  `main` baseline (report-only; see `benches/README.md`).
  `docs/ARCHITECTURE.md` section 7 now quotes measured numbers.
- **Multi-resource admission (cr-23, scenario S-19).** A pool can limit the
  units of each `ResourceKind` it holds at once: `resource_limits` in
  `PoolConfig` and `WorkerPoolConfig` (e.g. `{"gpu_vram": 24, "cpu": 4}`),
  `ResourcePool::with_resource_limit`, `DurableOptions::with_resource_limit`.
  A task is admitted only when its cost fits `max_units` (now the total over
  every kind) and every limited kind at once; a task needing 8 units of VRAM
  and one CPU slot waits while VRAM is free but no slot is. A task over any
  one limit is rejected with `CapacityExceeded` (requested and available are
  that dimension's). A pool without `resource_limits` admits exactly as
  before. Skip-ahead uses the same fit, and its starvation bound is
  unchanged. Validation rejects a kind limit of zero or above `max_units`.
  Per-kind usage: `ResourcePool::active_units_of` and native
  `WorkerPool::active_units_of`. Lifecycle events are unchanged. Checked by
  the `loom_vector` model (`--cfg parking_lot_negative_control="s-19"`
  checks only the total and must fail) and `vector_*` property tests on both
  in-memory pools.
- **Dependency and semver gates (cr-39).** CI job `udeps` (cargo-shear and
  cargo-udeps, native and wasm32) fails on unused dependencies; job `semver`
  runs cargo-semver-checks against the newest `v*.*.*` release tag and is
  inactive until the first one (`v0.3.0`) exists.
- **Skip-ahead bin-packing with a starvation guard (cr-20, scenario S-20).**
  When the head of the parked queue does not fit the free units, the
  dispatcher starts the first parked task, in priority-then-FIFO order, that
  does. After a head has been skipped `max_head_skips` times the dispatcher
  reserves the capacity for it and starts nothing else until it has started,
  so a head starts within `max_head_skips` dispatch cycles of becoming the
  head, and no later than the drain after the last completion among the
  tasks running when it became the head and the at most `max_head_skips`
  tasks started in its place (proof in the `dispatch::dispatcher` module
  docs). Applies to `ResourcePool` (looking at most `ADAPTER_LOOKAHEAD`, 64,
  parked tasks deep), the native `WorkerPool` and the WASM `WorkerPool`; a
  `DurableResourcePool` keeps strict order (it holds one leased task waiting
  for capacity, so there is nothing to skip to). Counters:
  `ResourcePool::skip_ahead_stats`, `WorkerPool::skip_ahead_stats`
  (native), `Dispatcher::skip_ahead_stats`. Skips are not lifecycle events.
  New in the core crate: `ParkedQueue::fitting`/`take` (default: no
  skip-ahead), `UnitSemaphore::available_units`, `DEFAULT_MAX_HEAD_SKIPS`,
  `SkipAheadStats`, `ADAPTER_LOOKAHEAD`. `PriorityQueue` is now an ordered
  map, and `take_all` returns items in dispatch order. Benchmark:
  `cargo bench --bench utilization` (utilization 0.947 with the default
  against 0.917 under strict order on a 95%-loaded mixed workload).
- **Stream replay (cr-33, scenario S-33).** `IggyStreamSink` publishes a task's
  streamed output (for example LLM tokens) to the `stream.chunks` topic as
  numbered chunks plus an end marker carrying the final `StreamOutcome`, keyed
  by the idempotency key; `IggyStreamReader::resume(key, ResumePoint)` gives a
  reconnecting client exactly the chunks after the last sequence number it saw,
  in order, then live ones until the end marker. Chunk ids are UUIDv5 values of
  key and sequence number; the cursor skips repeats and reports a missing chunk
  as `StreamError::Gap`. Unlike lifecycle events, chunks are never dropped on a
  full queue: `StreamWriter::send` waits for room (backpressure to the
  executor, off the dispatch path), and a chunk abandoned after its retries
  poisons the writer. New option `IggyOptions::stream_topic`.
- **Apache Iggy events and notifications (cr-32).** New crate
  `prometheus-parking-lot-iggy`, behind the facade's `iggy` feature
  (`infra::events`): `IggyHook` publishes every lifecycle event to a `lifecycle`
  topic partitioned by tenant (or pool), and `IggyNotifier` publishes
  `mailbox.ready` after a result is stored (used through `BackgroundNotifier` in
  a `StorageMailbox`). Message ids are UUIDv5 values of the idempotency key.
  Publishing is off the dispatch path through a bounded queue: `on_event` only
  enqueues, and a slow or down server causes bounded buffering and counted
  drops, never dispatch latency. The `iggy` client is pinned to `=0.11.0`; the
  tests run against `apache/iggy:0.9.0` (digest-pinned). New config backend
  `"type": "iggy"` (`config::IggyBackendConfig`, example
  `examples/configs/iggy.json`); `LifecycleEvent::pool()` is new in core.

- **Release hook for dropped permits (core).** `Dispatcher::install_release_hook`
  and `has_release_hook`: a permit dropped other than through
  `Dispatcher::complete` (a dropped `Grant`, `Dispatched` or `Admission`, or a
  task future the spawner discarded) returns its units and then calls the
  hook, which should schedule a `Dispatcher::poke`. `ResourcePool` and
  `DurableResourcePool` install one that spawns the poke through their
  spawner. `ParkedQueue::supports_skip_ahead` (default `false`) tells the
  dispatcher a queue implements `fitting`/`take`; `PriorityQueue` and
  `TaskQueueAdapter` return `true`.
- **Lifecycle events (cr-31, scenario S-21).** `ResourcePool`, the native
  `WorkerPool` and `DurableResourcePool` emit `Submitted`, `Queued`,
  `Started` and exactly one terminal event per task (`Completed`, `Failed`,
  `Rejected`, `Cancelled`, `Expired`, or `Requeued` for a durable task handed
  back to its queue), in that causal order for every observer. For
  `DurableResourcePool` the guarantee is one terminal event per run: a run
  ending in `Requeued` is followed by the task's next run. Register hooks
  with `ResourcePool::with_hook`/`WorkerPool::with_hook` (or on `hooks()`),
  `DurableOptions::with_hooks`, or
  `build_pools_from_scheduler_config_with_hooks`. Emission never blocks the
  scheduling path: events go through a bounded queue
  (`DEFAULT_HOOK_QUEUE_CAPACITY`, 4096) to a delivery thread; a full queue
  drops the event and counts it (`HookBus::dropped_events`), and a panicking
  hook is contained and counted (`HookBus::hook_panics`). With no hook
  registered an emission is one atomic load and a branch and builds no event.
  `Dispatcher::submit_with` reports a park under the dispatch lock, which is
  what orders `Queued` before `Started`.

- **Shutdown without silent loss (cr-29, scenario S-9).** Every pool has
  `shutdown_with(ShutdownPolicy) -> ShutdownReport` (`ResourcePool` and
  `DurableResourcePool`: async; `WorkerPool`: blocking `shutdown_with` and
  async `shutdown_with_async`). Submissions are refused at once; under
  `Drain { deadline }` and the default `DrainThenCancel { deadline }` accepted
  work keeps running (in-memory pools keep starting queued tasks) until the
  deadline; then queued tasks, and except under `Drain` running stragglers,
  are cancelled. `CancelImmediately` drains for zero time. In the in-memory
  pools each cancelled task's mailbox or result slot gets
  `TaskError::Cancelled` and a `CancelledTask` goes to the pool's
  `CancellationSink` (`with_cancellation_sink`, default
  `LoggingCancellationSink`), each call bounded by
  `CancellationConfig::delta` (`with_cancellation_config`); the native
  `WorkerPool` calls the sink from a dedicated thread. The durable pool stops
  leasing at once, hands back (`nack(Requeue)`) the leased task waiting for
  capacity and every cancelled straggler, and the next pool on the queue runs
  them. `ShutdownReport` (top level and `core::shutdown`) counts `completed`,
  `cancelled` (= `recorded + requeued + dropped + unanswered`) and
  `unsettled` tasks. `ShutdownPolicy::drain_deadline` and
  `cancels_stragglers` are public. Fixes the native `WorkerPool` path where a
  task received after the shutdown flag was dropped with no capture (0.2 dropped
  it with no result at all); the negative control
  `--cfg parking_lot_negative_control="s-9"` restores it and the `shutdown_`
  tests detect the loss. Tested by `-E "test(shutdown_)"`: every policy on
  `ResourcePool` and `WorkerPool`, and on `DurableResourcePool` over redb and
  Postgres (Docker, `--run-ignored all`; in the CI `postgres` job on PG 17 and
  18) with a restart that runs every handed-back task exactly once.

- **Durable dispatch (cr-49).** `DurableResourcePool` (top level, and
  `prometheus-parking-lot-runtime-tokio::durable_pool`) is the crash-durable
  mode of `ResourcePool` on any `LeaseQueue` and `ResultMailbox`:
  `DurableResourcePool::start(limits, queue, mailbox, executor, spawner,
  DurableOptions::new(lease_for))`. `submit` returns `Queued` only after
  `LeaseQueue::enqueue` has stored the task. A dispatcher leases the queue's
  head while a unit is free and admits it through the same `Dispatcher` and
  `UnitSemaphore` as the in-memory pool (capacity, priority then FIFO). Every
  leased task is heartbeated with `extend` (every third of the lease by
  default) until its lease is resolved, so a task that runs longer than the
  lease is not redelivered. The outcome is delivered to the mailbox with an
  idempotency key (the task's own key, or `"<queue identity>:entry:<queue
  entry id>"`), and only then is the lease acked (success) or dead-lettered
  (executor error, panic, timeout, expired deadline); a task too large for the
  pool is requeued for another consumer. A process
  killed at any point leaves the task leased; it is redelivered after the lease
  expires and the first-write-wins mailbox keeps exactly one result. A lost
  lease does not cancel the running execution; its delivery is deduplicated.
  The executor itself is at-least-once. In configuration, a pool whose `queue`
  and `mailbox` both name a backend is built as a durable pool
  (`BuiltScheduler::durable_pools`, `ConfiguredDurablePool` with `submit`,
  `consume`, `shutdown`); one backend drives at most one pool, and a pool with
  only one half on a backend is rejected. The `redb.json` and `postgres.json`
  examples now run their pool durably. Scenario S-16
  (`-E "test(pool_sigkill_)"`, `tests/scheduler/`) kills a pool process with
  `SIGKILL` mid-execution and between execution and the mailbox write, on PG 18
  and redb, and requires the task to complete with exactly one result after a
  restart; under `--cfg parking_lot_negative_control="s-16"` (ack before the
  mailbox write) the second case loses the result and fails.
- **Pools and backends from configuration (cr-37).**
  `builders::build_pools_from_scheduler_config` validates a `SchedulerConfig`,
  opens every backend (Postgres: connect, optional migration of the queue and
  mailbox schemas, `PgLeaseQueue`, `PgResultMailbox` and an optional
  `QueueListener`; redb: `EmbeddedStore`, `RedbLeaseQueue`,
  `RedbResultMailbox`) and builds every pool. A pool whose `queue` and
  `mailbox` are `"in_memory"` becomes a `ResourcePool` with a readable
  `SharedInMemoryMailbox`; a pool whose `queue` and `mailbox` both name a
  `postgres` or `redb` backend becomes a crash-durable `DurableResourcePool`
  on it (cr-49). One example per backend is in
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
  `infra::queue` and `InMemoryResultMailbox` in `infra::mailbox`. `DurableResourcePool`
  runs on these contracts (durable dispatch, cr-49). The redb backends (cr-28,
  below) and the Postgres ones (cr-26, cr-27, cr-30) implement them.
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
  `migrate(pool)` applies it. `DurableResourcePool` runs on it (cr-49).
- **Postgres result mailbox (cr-27).** `prometheus-parking-lot-pg::PgResultMailbox`
  is an async `ResultMailbox` on raw sqlx 0.9. Consume is one `DELETE ... RETURNING`
  with `FOR UPDATE SKIP LOCKED`, so a result reaches exactly one consumer.
  Duplicate deliveries keep the first write via `INSERT ... ON CONFLICT`. TTLs use
  database time, with `evict_expired` and a background sweeper (`spawn_sweeper`).
  Migration `005_result_mailbox.sql`; `infra::PgPool` is re-exported.
- **pgmq lease queue (cr-30).** `prometheus-parking-lot-pg::PgmqLeaseQueue` is an
  async `LeaseQueue` over the pgmq extension (>= 1.8, tested on 1.13.1, PG 17 and
  18) through plain SQL on raw sqlx 0.9; no pgmq client crate. One pgmq queue per
  priority level, the visibility timeout is the lease, `read_ct` is the fence
  (a stale token gets `LeaseLost`), nack-with-delay is `set_vt`, dead letters go
  to a per-queue pgmq dead-letter queue, and idempotency keys are first-write-wins
  in a small side table (`006_pgmq_lease_queue.sql`). Enqueue sends a `NOTIFY`
  (`PgmqLeaseQueue::listener()`); expiry and delays are polled. Differences from
  `PgLeaseQueue` (priority is read lane by lane, one-second lease granularity, no
  reaper) are listed in the module docs.
- **pgmq config backend (cr-30, cr-37).** A `"type": "pgmq"` backend section
  (`config::PgmqBackendConfig`: `url`, `queue_name`, `max_depth`, `lease_secs`,
  optional `max_attempts`, `listen`, `prepare`) opens a `PgmqLeaseQueue`
  (prepared when `prepare` is set) and an optional `QueueListener`, returned in
  `BuiltScheduler::pgmq` as `builders::PgmqBackend`. It is a queue for the
  application to use directly: validation rejects a pool whose `queue` or
  `mailbox` names a pgmq backend, because pool-level crash durability on pgmq is
  not claimed (use a `postgres` backend for a durable pool). The queue name is
  validated with the rule `PgmqLeaseQueue::new` enforces. Example:
  `examples/configs/pgmq.json`. `infra::queue::PgmqLeaseQueue` re-exports the
  queue.

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
  `infra::mailbox::RedbResultMailbox`. `DurableResourcePool` runs on them
  (cr-49); `examples/embedded_pool.rs` shows direct use.

- **Composable mailbox with a notifier (cr-34).** `StorageMailbox<S, N>` is now a
  `ResultMailbox`: it stores in `S` first and notifies only after a `Stored`
  outcome (a first-write-wins `Duplicate` does not notify). The notification runs
  off the delivery path through `mailbox::BackgroundNotifier`, which spawns it on a
  `Spawn`er and retries a failing notifier with capped exponential backoff
  (`mailbox::RetryPolicy`, a `mailbox::Sleeper` for the wait). A notifier failure is
  logged and counted (`NotifyStats`), never fails or undoes a stored result. Core
  stays runtime-free.
- **WASM pool parity (cr-53).** The WASM `WorkerPool` emits the lifecycle
  events of the native pools (`Submitted`, `Queued`, `Started`, exactly one
  terminal event) through the core `HookBus` (`with_hook`, `hooks`; inline
  delivery, timestamps from the pool's clock), exposes `skip_ahead_stats`, and
  is covered by `wasm-bindgen-test` for per-dimension admission, bounded and
  strict skip-ahead, and the S-21 sequence for completed, failed, timed-out,
  rejected, expired and shutdown-cancelled tasks. Durable dispatch does not
  apply on the web target (no durable backend in the browser) and a panic
  aborts the module; both are documented in the crate docs.
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

- **Postgres 17 and 18 matrix, autovacuum, runbook (cr-36).** CI gains a
  `postgres` job that builds `parking-lot-pg:17` and `:18` (buildx, GitHub
  Actions layer cache) and runs every Docker-gated Postgres test on both: the
  `parking-lot-pg` suites (lease queue, mailbox, pgmq, image smoke) and the
  facade's Postgres config test. The suites read `PARKING_LOT_PG_MAJOR`
  (default 18); before, only the pgmq suite did. The queue tables get per-table
  autovacuum settings (scale factor 0 with fixed thresholds of 500 to 1000 dead
  tuples) from migrations 004 to 006, and `PgmqLeaseQueue::prepare` applies them
  to pgmq's own `q_*` tables through the new `pl_pgmq_tune_queue` function. See
  `docs/runbooks/postgres18.md` (PG 18 data checksums by default, the Docker
  volume and `PGDATA` change, SCRAM, pgmq install, upgrading 17 to 18, bloat
  monitoring).
- **Loom models in CI, with their negative controls.** A `loom` job in
  `.github/workflows/ci.yml` runs every loom model (`loom_capacity`,
  `loom_wakeup`, `loom_vector`, `loom_skip_ahead`, `loom_at_depth`) under
  `--cfg parking_lot_loom` with `LOOM_MAX_PREEMPTIONS=3`. One leg per
  negative control (S-1, S-2, S-19, S-20, S-20-depth, S-20-order) requires the models
  that control breaks to fail, so a model that stops detecting its defect
  fails the build. Each leg is one cfg build with a cache of its own. The new
  `loom_at_depth` model uses `max_queue_depth = 1`, so submissions behind a
  blocked head go through the at-depth admission path
  (`Dispatcher::start_at_depth`), which `loom_skip_ahead` (depth 8) never
  reached. It races those submissions against a completion, and against a
  permit dropped outside the dispatch lock, and checks capacity, no lost
  wakeup, the starvation bound and priority order. The new negative control
  `--cfg parking_lot_negative_control="s-20-depth"` stops charging at-depth
  skips to the head, and `"s-20-order"` lets an at-depth submission start
  ahead of a higher-priority head that fits; only `loom_at_depth` catches
  them. Every model now runs two loom passes, with and without a yield
  inside a running job's window (`loom_support::model`): with the yield only,
  loom never scheduled a completion between two dispatch-lock sections of a
  thread that kept running, and each pass alone missed lock orders the other
  reached.

### Removed

- **`migrations/003_install_pgmq.sql`.** Nothing applied it, and its
  `COMMENT ON EXTENSION` failed when pgmq was absent. `PgmqLeaseQueue` installs
  the extension through `006_pgmq_lease_queue.sql`.
- **The sea-orm Postgres backends (cr-27).** The synchronous `PostgresQueue` and
  `PostgresMailbox`, the sea-orm entities (`infra::entities`), and migrations 001
  and 002 are removed (existing `pl_queue_jobs` / `pl_mailbox_messages` tables
  are not dropped). sea-orm and sqlx 0.8 are gone from the dependency graph; the
  `postgres` feature now uses raw sqlx 0.9 only.
- **The JSONL file queue and mailbox (cr-28).** `YaqueQueue`, `YaqueMailbox`
  and the facade paths `infra::queue::yaque` and `infra::mailbox::yaque` are
  removed; the redb backends replace them. They were not crash-safe, and their
  files are not migrated. `examples/yaque_pool.rs` is now
  `examples/embedded_pool.rs`.
- **Unused dependencies (cr-39).** Dropped from the manifests: the `chrono` and
  `testcontainers-modules` entries of `[workspace.dependencies]` (no member used
  them) and the `tracing-subscriber`
  dev-dependency of `prometheus-parking-lot-runtime-tokio`. Build-only: no public
  API or feature changes. `Cargo.lock` shrinks from 518 to 507 packages.
- **Duplicate `reqwest` and `flume` (cr-39).** The runtime crate's test-only
  `reqwest` (0.12) and `flume` (0.11) move to 0.13 and 0.12, the versions the
  Iggy client already builds. This removes `native-tls`, `openssl-sys` and
  `hyper-tls` from the lockfile.

### Fixed

- **Iggy personal access token printed by `Debug`.** A connection
  string `iggy://<token>@host:8090` (one credential, no colon) is now masked
  whole in `IggyOptions` and `IggyBackendConfig` `Debug`. Before, only userinfo
  with a colon was masked, so the token was printed in full.
- **`?password=` in a Postgres URL printed by `Debug`.** The value
  of any `password` query parameter is masked in `PostgresBackendConfig` and
  `PgmqBackendConfig` `Debug`.
- **Iggy message ids collided when task ids repeat.** Task ids are
  caller-supplied (the README uses 0 for every task), so lifecycle ids built
  from pool, kind, task id and millisecond collided, and the publisher's repeat
  window dropped real events as "deduplicated". A lifecycle event now has its own
  id and bypasses the window; a `mailbox.ready` without an idempotency key gets a
  distinct id per notification. `TaskMetadata.id` is documented as caller-supplied.
- **Iggy streams assumed the configured partition count.** A stream
  topic that already existed with another count made writes fail and readers wait
  forever. The sink and reader now read the topic's real partition count.
- **An invalid Iggy `connection_string` loaded and then lost every event.**
  It is now parsed with the Iggy client's parser during validation.
- **Docs overclaimed the stream offset hint**: a hint past the end
  of a stream waits instead of reporting an error. `streaming-replay.md` and the
  `stream` module say so.
- **`QueueFull` no longer refuses work that can start now (cr-20).** With
  `max_queue_depth` tasks parked, a submission that would start at once is
  admitted instead of refused: one with a strictly higher priority than the
  parked head that fits, or one that fits beside a blocked head while the
  starvation guard still allows a skip (it counts as a skip). Priority order
  and the `max_head_skips` bound are unchanged; a task that would have to
  wait still gets `QueueFull`.
- **A dropped grant no longer strands parked work.** A permit returned other
  than through `Dispatcher::complete` freed its units but drained nothing, so
  parked work it would admit waited for the next submission or completion,
  possibly forever (for example after a spawner discarded a task future).
  It now calls the dispatcher's release hook (see Added), which the tokio
  pools use to poke; the native and WASM `WorkerPool`s return every permit
  through `complete` or poke after a panic.
- **A cost whose units overflow is rejected.** A `ResourceCost` whose
  per-kind units sum past `u32::MAX` (its total saturates) was admitted by a
  pool of `u32::MAX` units; it is now `CapacityExceeded` (`kind: None`).
- **The drain cannot spin on a vanishing head.** If a `ParkedQueue` reported
  an expired head and then could not pop it, the drain looped forever; it now
  stops, as it already did for a head that fits.
- **`BackgroundNotifier::with_policy` always applies.** It was silently
  ignored once a dispatched notification still held the notifier's shared
  state; the policy now applies to every notification dispatched after the
  call.
- **One deadline rule.** `InMemoryQueue::prune_expired` removed a task whose
  deadline equalled `now`, while the dispatcher (`Ticket::is_expired`) and the
  pools treat it as live until `now > deadline`. It now keeps it, and
  `TaskQueue::prune_expired` and `TaskMetadata::deadline_ms` document the rule.
- **Documentation.** The hook docs no longer promise isolation on `wasm32`:
  there, hooks run inline on the scheduling path (`Queued` under the dispatch
  lock, so a hook must not call back into the pool) and a panicking hook
  aborts the module. The event-sequence docs now say that tasks removed by
  `prune_expired` emit no terminal event (issue #57, 0.4.0). The
  `loom_skip_ahead` model now asserts the starvation bound itself
  (`skipped <= max_head_skips`) and that the guard engaged, and fails under
  the S-20 negative control.
- **Durable exactly-one-result no longer depends on pool names.**
  Differently named pools consuming one queue stored a second result when a
  consumer died between delivery and ack, and pools on different queues
  sharing one mailbox dropped each other's results as duplicates (entry ids
  repeat across queues). Results are now keyed by the queue's identity.
  Tested by `durable_differently_named_consumers_of_one_queue_store_one_result`,
  `durable_pools_on_two_queues_share_one_mailbox_and_keep_both_results` and
  the S-16 `pool_sigkill_*_between_delivery_and_ack` tests, whose restarted
  pool is named differently.
- **Durable `ShutdownReport`.** A task is counted when its lease is
  resolved: `completed` once its outcome is stored, `requeued` (and
  `cancelled`) when it is handed back, including a task whose delivery kept
  failing (it was reported `completed`). A task still delivering when the
  shutdown began is counted when it lands (it was counted nowhere), and the
  dispatcher no longer counts as an `unsettled` task.
- **`ResourcePool` lost woken tasks on a panicking `Mailbox` or `AuditSink`.**
  A completion now starts the tasks it woke before it calls them.
- **Durable pool had no bound for zero-cost tasks.** New
  `DurableOptions::with_max_running` (default: `max_units`) bounds the tasks
  running at once.
- **Durable cancellation.** A running task cancelled at shutdown whose
  executor panics while being dropped is handed back, not dead-lettered as
  `Failed`.
- The S-21 negative control also suppresses the durable `Requeued` event, so
  the durable requeue tests are covered by it.
- **One undecodable task wedged the whole embedded queue.** `RedbLeaseQueue::lease`
  aborted its transaction on a stored task that did not decode as `P` (for
  example after an app update changed the payload type), leaving the entry at the
  front so every later lease failed the same way. The entry is now moved to the
  dead letters with the decode error as its reason, in the same transaction, and
  leasing continues; the Postgres queue already behaved this way.
- `RedbResultMailbox::consume` of an entry that no longer decodes now removes it
  and returns the decode error once, as the Postgres mailbox does, instead of
  failing on every call for that key. `peek` still only reads.
- `WorkerPool::retrieve_async` no longer uses a blocking-pool thread per
  waiter (cr-22); any number of concurrent waiters fit on a runtime whose
  blocking pool is exhausted.
- `TokioSpawner::with_worker_threads` keeps its runtime alive for as long as
  the spawner or any clone exists (cr-19). In 0.2 the runtime was dropped when
  the constructor returned, so every spawn went to a dead runtime.
  `TokioSpawner` now also implements `Debug`.

- The WASM pool ignored `max_units`; it now enforces capacity,
  priority then FIFO order and the queue depth (cr-38).
- **`PgmqLeaseQueue`: a nacked token could still be used (cr-30).** After
  `nack(Requeue { delay > 0 })` the old lease token passed the fence guard again
  (the delay sets the visibility timeout without changing `read_ct`), so
  `ack(old)` deleted the requeued task and `extend(old)` kept it hidden. The
  nack now records the end of the delay in `pl_pgmq_aux` in the same statement
  and the guard rejects a token while a delay runs; every operation is still one
  atomic statement. The shared `LeaseQueue` conformance suite gains
  `nacked_token_is_fenced_during_delay` (all backends), and
  `enqueue_and_lease_round_trip` now also asserts `meta.sequence`.
- **Conformance cases robust on a loaded machine.** `leased_task_is_invisible`
  and `extend_keeps_lease` use leases much longer than the time they let pass, so a
  stalled real-clock backend cannot expire a lease between two calls.
- **Migrations took blocking locks on every start (cr-26, cr-27, cr-30,
  cr-36).** `CREATE INDEX IF NOT EXISTS` (SHARE) and `DROP TRIGGER` /
  `CREATE TRIGGER` (ACCESS EXCLUSIVE) ran on every call to `migrate`, and
  `PgmqLeaseQueue::prepare()` re-ran `pgmq.create`. They are now guarded by
  catalog checks (`CREATE OR REPLACE TRIGGER` when a trigger is missing), each
  script sets `lock_timeout = 5s`, and `ALTER TABLE ... SET` runs only when the
  options differ. A re-run no longer queues behind an open transaction.
- **`PgLeaseQueue` `lease` and the reaper could wait on, or deadlock over, row
  locks (cr-26).** The attempts-exhausted sweep inside `lease` and the reaper
  now pick rows with an ordered `FOR UPDATE SKIP LOCKED` sub-select.
- **A failed dead-letter write discarded tasks already leased (cr-26, cr-30).**
  `lease` now logs the failure and returns the tasks it leased; the entry that
  could not be dead-lettered stays leased until its lease expires.
- `QueueListener::wait` no longer claims to be cancel safe: sqlx does not
  promise that a cancelled receive keeps a notification. Callers already poll
  on `Wake::TimedOut`.
- Release packaging: `cargo package --workspace` failed for
  `prometheus-parking-lot-embedded` (a self dev-dependency that carried a
  version), and so for the facade, which depends on it; the six library crates now ship a README and the
  MIT license file; workspace version is 0.3.0.
- Building the facade for `wasm32-unknown-unknown` needs
  `default-features = false`: the default `tokio-runtime` feature does not
  build for that target. README, `docs/API.md` and the crate docs say so, and
  CI checks it.
- **Credentials holding `?`, `#`, `/`, `@` or `:` were printed by `Debug`.** The
  redaction of Postgres, pgmq and Iggy connection strings split at the first
  `?` or `/` before it found the userinfo, so `iggy://app:pa?ss@iggy:8090`
  printed its password. The userinfo now ends at the last `@`, found before
  any query string is read.
- **Validating an `iggy+quic://` connection string built a client.** It did a
  blocking name lookup, failed outside a tokio runtime and bound a UDP socket
  inside one. `validate_connection_string` now parses with the Iggy
  connection-string grammar for the string's transport and builds nothing.
- **A configuration could name one queue twice, or use an empty name.** Two
  `postgres` or two `pgmq` sections with the same database and `queue_name`,
  or two `redb` sections with the same `path`, are rejected (the error names
  both sections); the comparison is lexical and cannot see symlinks or a host
  name versus its IP address. A pool or backend name must be 1 to 64
  characters of ASCII letters, digits, `-` and `_`; an empty pool name used to
  pass validation and fail after the backends were opened.
- Docs: the message-id description for Iggy now matches the code (keyed
  `mailbox.ready` ids and stream chunk ids are derived from the key; keyless
  `mailbox.ready` and lifecycle ids use a per-instance nonce and sequence), the
  Iggy 0.9 de-duplication note says "server", the `listen` setting says the
  durable pool still polls (#54), and `docs/API.md` lists `pgmq` and the
  per-field feature gates of `BuiltScheduler`.

### Known limitations

- Only `ResourcePool` has a durable mode (cr-49). The `WorkerPool` (native and
  WASM) and in-memory `ResourcePool`s still lose parked work on restart.
- Durable dispatch is at-least-once for the executor: a task can run again
  after a crash, a lost lease or a failed ack. Results are deduplicated by the
  mailbox; executors with external side effects must be idempotent.
- A durable queue dropped and recreated under the same identity (a Postgres
  table or pgmq queue whose id sequence restarts) can reuse entry ids; a
  mailbox that remembers the old keys would then drop the new results as
  duplicates. Recreate the mailbox with the queue, or set task idempotency
  keys.
- After a crash between storing a durable task's `Failed` result and
  dead-lettering it, the redelivered run can emit `Completed` while the stored
  result stays `Failed` (first write wins); events report the run, the
  mailbox the task.
- A durable pool lets at most one leased task wait for capacity; a task
  enqueued later at a higher priority waits behind it instead of overtaking it
  as it would in the in-memory pool.
- An idle durable pool notices work enqueued by another process, and leases
  that expired, by polling (`DurableOptions::with_poll_interval`, 500 ms by
  default); it does not use the Postgres `QueueListener` yet.
- Lifecycle events are lossy under backpressure: a hook that cannot keep up
  makes the bus drop events (counted), so an observer can miss a terminal
  event. Tasks removed by `ResourcePool::prune_expired` emit no terminal
  event (the `TaskQueue` contract does not return their ids).

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
