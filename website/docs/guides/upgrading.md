---
title: Release and upgrade notes
description: What 0.3.0 is, how to depend on it before and after it reaches crates.io, and the source changes a 0.2 user must make, item by item.
---

# Release and upgrade notes

0.3.0 is the first release published to crates.io. It is source-breaking for
code written against 0.2, which was used from the repository. This page is the
checklist; the [changelog](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/CHANGELOG.md)
has every change with the OpenSpec change that made it.

## Depending on it

Depend on the version and enable the backends you use:

```toml
[dependencies]
prometheus_parking_lot = { version = "0.3", features = ["postgres", "embedded", "iggy"] }
```

| Feature | Enables | Default |
| --- | --- | --- |
| `tokio-runtime` | `ResourcePool`, `DurableResourcePool`, native `WorkerPool`, shutdown, cancellation, builders | yes |
| `postgres` | `PgLeaseQueue`, `PgmqLeaseQueue`, `PgResultMailbox`, `PostgresAuditSink` | no |
| `embedded` | `EmbeddedStore`, `RedbLeaseQueue`, `RedbResultMailbox` | no |
| `iggy` | `IggyHook`, `IggyNotifier`, `IggyStreamSink`, `IggyStreamReader` | no |

The facade re-exports the layered crates under the paths 0.2 used (`core::*`,
`infra::*`, `config::*`, `runtime::*`, `util::*`), so most imports keep
working. Toolchain: MSRV 1.95, edition 2024.

## Source changes, in the order the compiler will report them

| 0.2 | 0.3 |
| --- | --- |
| `TaskExecutor::execute` and `WorkerExecutor::execute` return `T` | They return `Result<T, TaskError>`. Wrap success in `Ok`; return `TaskError::executor(message)` for a failure the client should see. |
| `ResourceCost { kind, units }` struct literal | `ResourceCost::new(kind, units)`, and `.with(kind, units)` for a second kind. Serialized costs in the 0.2 form still load. |
| `TaskMetadata { .. }` without `sequence` | Add `sequence: 0`; the pool stamps the submission order. |
| `PoolError` | Removed. `WorkerPool` returns `SchedulerError`; `QueueFull` now carries `pool`, `depth`, `capacity` and `retry_hint`. |
| `TaskStatus::Failed(String)` | `TaskStatus::Failed(TaskError)`. `TaskError`, `SchedulerError`, `TaskStatus` and `ResourceKind` are `#[non_exhaustive]`: add a `_` arm to matches. |
| `ResourcePool::submit(task, now_ms)`, `prune_expired(now_ms)` | No time argument; time comes from a `Clock` (`with_clock`, `TestClock` in tests). |
| `PoolConfig` / `WorkerPoolConfig` struct literals | Add `max_head_skips` and `resource_limits` (or deserialize them). |
| `runtime` field and `RuntimeConfig`, `QueueBackendConfig`, `MailboxBackendConfig` in the config | Removed. Unknown fields now fail to load. `queue` and `mailbox` are `"in_memory"` or `{ "backend": name }`; durable stores go under `backends`. |
| `validate` / `from_json_str` return `String` errors | They return `ConfigError` with the field's dotted path. `build_pools` returns a `BTreeMap`. |
| sea-orm `PostgresQueue`, `PostgresMailbox`, `infra::entities` | Removed. Use `PgLeaseQueue` and `PgResultMailbox` (raw sqlx 0.9), or a `postgres` backend. Old tables are not dropped or migrated. |
| `YaqueQueue`, `YaqueMailbox` (JSONL files) | Removed. Use the `embedded` feature's redb store. Old files are not migrated. |
| `CancelledTask::payload: P` | `Option<P>`: a running task cancelled by the native `WorkerPool` has none. |
| `HookBus` called hooks inline; `register(&mut self)` | Hooks run on the bus's own thread; `register(&self)`; `HookBus::flush(timeout)` waits for delivery. |

## Behaviour that changes without a compile error

- **Admission is enforced everywhere.** The native `WorkerPool` and the WASM
  pool now honour `max_units` (0.2 ignored it), and a task costing more than a
  pool's capacity, or than any per-kind limit, is rejected with
  `CapacityExceeded` at submission instead of parking forever.
- **Skip-ahead is on by default.** A smaller parked task that fits may start
  ahead of a larger one that does not, up to 8 times per head. Set
  `max_head_skips: 0` for 0.2's strict order.
- **`WorkerPool` budgets.** Every execution is bounded by its deadline or
  `default_timeout_ms`; a task still running at its budget is cancelled and
  `retrieve` returns `TaskError::Timeout`. Raise `default_timeout_ms` to the
  longest run you mean to allow.
- **Executor panics are contained.** The outcome is
  `TaskError::ExecutorPanicked` and the units are released (native pools; WASM
  still aborts).
- **Blocking `WorkerPool::retrieve` panics on an async thread.** Use
  `retrieve_async`, or call it from `spawn_blocking`.
- **A timed-out `retrieve` keeps the result** for a later call, until the
  result TTL evicts it.
- **`WorkerPool::shutdown()` hands queued tasks to the cancellation sink** and
  waits about 2 s in total for workers.

## New in 0.3.0

Durable pools on Postgres and redb ([Cloud Postgres](./cloud-postgres.md),
[Tauri local LLM](./tauri-local-llm.md)), per-resource admission and
skip-ahead ([GPU inference pool](./gpu-inference-pool.md)), shutdown policies
with reports ([Shutdown and requeue](./shutdown-and-requeue.md)), lifecycle
events and Iggy publishing ([Iggy events](./iggy-events.md)), stream replay
([Streaming replay](./streaming-replay.md)), the strict configuration schema
([Configuration](./configuration.md)), and Kotlin, Swift and Tauri bindings
([`bindings/README.md`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/bindings/README.md)).

## Known limitations at 0.3.0

- Only `ResourcePool` has a durable mode. `WorkerPool` (native and WASM) and
  in-memory pools lose parked work on a crash.
- Durable execution is at-least-once; results are exactly-once.
- A durable pool dispatches strictly in order, holds at most one leased task
  waiting for capacity, and polls for work other processes enqueue.
- Lifecycle events are dropped (and counted) under backpressure on the native
  pools. The WASM pool calls its hooks inline (there are no threads there):
  a slow hook stalls it, a hook must not call back into the pool, and a
  panicking hook aborts the module. Tasks removed by `prune_expired` emit no
  terminal event (issue #57).
