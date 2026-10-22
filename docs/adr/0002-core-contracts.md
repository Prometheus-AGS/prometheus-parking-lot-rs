# ADR 0002: Core contracts for 0.3.0 — error taxonomy, fallible executors, clock, ordering, cost vector

## Status

Accepted (cr-18-core-contracts, phase-correctness-and-durability, milestone M1).

## Date

2026-10-22

## Context

Implementers of M1/M2 (dispatcher and semaphore cr-19, panic containment cr-21, result
slots cr-22, multi-resource admission cr-23, backend contracts cr-25, durable dispatch
cr-49) build against the core contracts. Before this change:

- executors returned a bare `T`, so a failed task was indistinguishable from a
  successful one and the pool could only report a budget kill (as
  `Failed(String)`);
- two error enums existed: `SchedulerError` (`ResourcePool`, queues, mailboxes) and
  `PoolError` (`WorkerPool`), with overlapping, unit-like variants;
- time was read from `SystemTime` inside the pool, so deadline behaviour could only be
  tested by sleeping;
- the in-memory queue ordered equal priorities by `created_at_ms` (millisecond
  resolution: ties came out in arbitrary heap order) while `Eq` compared ids, so `Eq`
  and `Ord` disagreed;
- `ResourceCost` was a scalar `{kind, units}`; multi-resource admission (cr-23) would
  have had to change the type every backend, config and FFI binding stores;
- `ResourcePool` threaded nine `Arc`s through boxed `'static` futures, and the public
  `sync_wake_worker_loop` reserved capacity for a dequeued task and then dropped the
  task.

## Decision

### Error taxonomy: one family, two enums

| Enum | Covers | Properties |
|---|---|---|
| `TaskError` (`#[non_exhaustive]`) | how one task's execution failed: `Executor { message, retryable }`, `ExecutorPanicked { message }`, `Timeout { budget }`, `Cancelled { reason }`, `DeadlineExpired` | plain data: `Clone`, `Eq`, `Serialize`/`Deserialize` (internally tagged `kind`), because it is stored in mailboxes and crosses processes |
| `SchedulerError` (`#[non_exhaustive]`) | admission (`QueueFull` with pool/depth/capacity/retry hint, `CapacityExceeded`, `DeadlineExpired`, `PoolShutdown`), `InvalidConfig`, `Backend` (with `#[source]`), `Timeout` (a wait for a result), `ResultNotFound`, `Internal`, and `Task(TaskError)` | `thiserror`, source chains; `Task` is `#[error(transparent)]` and `From<TaskError>` |

`TaskError` is separate because it is a value a task produces (stored, serialized,
compared) while `SchedulerError` is an operation's failure carrying non-`Clone` sources.

`PoolError` is removed. Mapping:

| 0.2 `PoolError` | 0.3 `SchedulerError` |
|---|---|
| `QueueFull` | `QueueFull { pool: "worker_pool", depth, capacity, retry_hint: None }` |
| `InsufficientCapacity { requested, available }` | `CapacityExceeded { pool, requested, available }` (never constructed by 0.2) |
| `Timeout` | `Timeout { task_id: None }` |
| `ResultNotFound` | `ResultNotFound { key: Some(<slot key>) }` |
| `PoolShutdown` | `PoolShutdown { pool: "worker_pool" }` |
| `InvalidConfig(msg)` | `InvalidConfig { message }` |
| `Internal(msg)` | `Internal { message }` |
| — (executor results were infallible) | `Task(TaskError)` |

`CapacityExceeded` now means "the cost exceeds the pool's total capacity, so the task can
never be admitted" and is not retryable; `ResourcePool` rejects such tasks at submission
(parked at the head of a FIFO queue they would block every task behind them).
`is_retryable` is true for `QueueFull`, `Timeout`, and `Task(e)` when the executor marked
`e` transient.

### Fallible executors

`TaskExecutor::execute` and `WorkerExecutor::execute` return `Result<T, TaskError>`.
`ResourcePool` releases capacity and then delivers `Completed` + value or
`Failed(TaskError)` + no payload; a budget kill is `Failed(TaskError::Timeout { budget })`.
The worker pools store the `Result` in the result slot, count failures in
`PoolStats::failed_tasks`, and `retrieve`/`retrieve_async` return the error as
`SchedulerError::Task`. Panics are still not caught (cr-21 adds `catch_unwind` and
produces `ExecutorPanicked`).

`TaskStatus` is `#[non_exhaustive]`, derives `PartialEq`/`Eq`, and `Failed` carries a
`TaskError` instead of a `String`.

### Clock

`Clock: Send + Sync + Debug` with `monotonic() -> Duration` (since the clock's origin)
and `now_ms() -> u128` (wall time, the unit of `deadline_ms`/`created_at_ms`, which
cross processes). `SystemClock` reads `std::time`; `TestClock` is advanced by the test
and shared by clones. The trait returns a `Duration` rather than an `Instant` because
`Instant` cannot be constructed deterministically and panics on
`wasm32-unknown-unknown`; the WASM crate supplies its own clock (cr-38).
`ResourcePool::with_clock` routes every time read (submit deadline check, execution
budget, head-of-queue expiry, `prune_expired`, audit timestamps) through the clock;
`submit` and `prune_expired` no longer take a `now_ms` argument.

### Ordering: monotonic submission sequence

`TaskMetadata::sequence: u64` (`#[serde(default)]`) is stamped by the pool from a
`SubmissionSequence` (an `AtomicU64` starting at 1; 0 means unsequenced) under the queue
lock at submission. The in-memory queue orders by
`(priority, Reverse(sequence), Reverse(created_at_ms), Reverse(id))`, and `Eq` compares
the same key, so `Eq` and `Ord` agree. The sequence travels in the metadata, so a task
that is dequeued and put back (head does not fit) keeps its exact place. Durable queues
order by their own monotonic column (cr-26); the field is a per-pool key, not a global
one.

`ResourcePool` claims capacity only while holding the queue lock, and a submission
starts immediately only when the queue is empty, so no submission can overtake a parked
task. This is strict priority-then-FIFO admission; deliberate skip-ahead is cr-20.

### Cost vector

`ResourceCost` is a vector of units per `ResourceKind` (private `BTreeMap`, zero
dimensions not stored). `ResourceCost::new(kind, units)` is the one-dimension
constructor; `with`, `units_of`, `total_units`, `dimensions`, `iter`, `From<(kind,
units)>` and `FromIterator` cover the rest. Serialized as a map
(`{"cpu":1,"gpu_vram":8}`); deserialization also accepts the 0.2
`{"kind":...,"units":...}` form so persisted 0.2 metadata loads. `ResourceKind` is
`#[non_exhaustive]` and `Ord` (the fixed acquisition order for cr-23).

Until cr-23, pools admit by `total_units()` against the scalar `max_units`, which is
exact for one-dimension costs. cr-23 changes admission and adds per-dimension limits to
config; it does not change this type.

Dimensions are the `ResourceKind` enum rather than free-form strings: enum keys
serialize as JSON object keys, stay `Copy`, and give a total acquisition order. If
deployments need open-ended dimensions (for example one per GPU), a `Named` variant can
be added under `#[non_exhaustive]`.

### `ResourcePool` shape

One `Arc<Inner>` holds limits, the atomic unit counter, the queue and mailbox mutexes,
executor, spawner, audit sink, clock, sequence and shutdown flag. A running task's
future owns an `Arc` clone; completion (release, deliver, audit, dispatch the queue) is
synchronous code on `Inner`. The spawner bound is `S: Spawn + Send + Sync + 'static`
(`Clone` is no longer required), which makes `submit`'s future `Send`.

`sync_wake_worker_loop` and `WakeState` are deleted: the loop reserved capacity for a
dequeued task and dropped it (silent task loss plus a capacity leak), and nothing could
call it correctly. `shutdown()` now stops new submissions (`PoolShutdown`); running and
queued tasks keep draining. Drain/cancel policy is cr-29.

## Consequences

- 0.3.0 is source-breaking for executors, `ResourceCost` literals, `TaskMetadata`
  literals (`sequence`), `submit`/`prune_expired` call sites, `PoolError` users and
  exhaustive matches on `TaskStatus`/`ResourceKind`. The migration is in
  `CHANGELOG.md` (0.3.0).
- Backends store `TaskStatus::Failed` with the `TaskError` display string (Postgres) or
  serialized (embedded); cr-27 defines the durable status schema.
- cr-19 builds the dispatcher and semaphore on `SubmissionSequence`, `Clock` and
  `total_units`; cr-21 produces `ExecutorPanicked`; cr-22 stores `Result<_, TaskError>`
  in result slots; cr-23 changes admission only; cr-25's async contracts reuse
  `SchedulerError`.

## Alternatives considered

- **Keep `PoolError` as an alias of `SchedulerError`.** Rejected: an alias with
  different variants still breaks every `match`, and two names for one type invite
  confusion.
- **`Box<dyn Error>` inside `TaskError::Executor`.** Rejected: the error must be stored,
  serialized and compared; executors put their full context in the message.
- **Queue-internal sequence assigned at enqueue.** Rejected: a dequeued-then-requeued
  head would lose its place.
- **Keep scalar `ResourceCost` until cr-23.** Rejected by the plan: backends, config
  and FFI would persist the scalar shape that cr-23 then breaks.
