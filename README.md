# prometheus_parking_lot

A resource-aware scheduler for expensive work such as LLM inference. Each task
declares what it costs (for example 4 units of GPU memory). A pool starts the
task when its cost fits in the capacity that is free and nothing is waiting
ahead of it; otherwise it parks the task and starts parked tasks, highest
priority first and in submission order within a priority, as running tasks
release their units. A task that costs more than the pool's total capacity is
rejected at submission instead of blocking the queue forever.

Every pool shares one admission core (a dispatcher and a unit-weighted
semaphore):

- **`ResourcePool`** runs async executors on a tokio runtime and delivers each
  outcome (value or error) to a **mailbox** keyed by tenant, user and session,
  so a client that disconnected can collect it later.
- **`WorkerPool`** runs executors on dedicated OS threads, each with its own
  single-threaded tokio runtime, so CPU- or GPU-bound work never blocks your
  async runtime. Results need not be serializable (a streaming channel is
  fine) and are collected with `retrieve`.
- **`DurableResourcePool`** is `ResourcePool`'s crash-durable mode: it parks
  tasks in a Postgres or redb lease queue and delivers results to a durable
  result mailbox, so accepted work survives a crash (cr-49).
- On **`wasm32`**, a `WorkerPool` runs executors as async tasks on the browser
  or Node event loop instead of threads (crate `prometheus-parking-lot-wasm`).

Every pool emits lifecycle events (submitted, queued, started, then one
terminal event; for the durable pool, one terminal event per run) to hooks you
register, off the dispatch path (cr-31).

Every execution runs under a budget: the task's deadline or the pool's default
timeout, whichever is nearer. Work still running at its budget is cancelled and
its units are released.

This README describes **0.3.0**, the first release on crates.io. 0.3.0 is
source-breaking for 0.2 users; the [CHANGELOG](CHANGELOG.md) has the
migration notes.

## Status: what holds today and what does not

| Guarantee | Today |
| --- | --- |
| Admitted units never exceed `max_units` | Yes, for every pool (`ResourcePool`, `DurableResourcePool`, native and WASM `WorkerPool`): one dispatcher and one unit-weighted semaphore, covered by loom and property tests. With per-kind limits, no limited kind exceeds its limit either. |
| Priority, then FIFO | Yes, among the tasks that fit. A smaller task that fits starts ahead of a large head that does not, at most `max_head_skips` times (default 8) before the head gets the capacity reserved (cr-20); `0` keeps strict head-of-line order. Durable pools dispatch strictly in order. |
| Execution budgets | Yes, for every pool (JS timers on WASM). |
| Multi-resource admission | Yes, for every pool (cr-23): give a pool per-kind limits (`resource_limits`, e.g. `{"gpu_vram": 24, "cpu": 4}`) and a task starts only when its cost fits `max_units` (the total) and every limited kind at once; a task needing 8 units of VRAM and one CPU slot waits while VRAM is free but no slot is. A task over any one limit is rejected with `CapacityExceeded`. Without `resource_limits`, admission is by the total alone. Covered by a loom model (`loom_vector`) and property tests. |
| Executor panics | Contained, for `ResourcePool`, `DurableResourcePool` and the native `WorkerPool` (cr-21): the outcome is `TaskError::ExecutorPanicked`, the task's units are released and parked work starts; a `WorkerPool` worker killed by a panic is replaced. On WASM a panic aborts the module (`panic=abort` on stable Rust). |
| Accepted work survives a crash | **Yes, for `ResourcePool` on a durable backend** (cr-49): configure a pool's `queue` and `mailbox` as `{"backend": "<name>"}` (or start a `DurableResourcePool` directly) on Postgres (`PgLeaseQueue`, `PgResultMailbox`) or redb (`RedbLeaseQueue`, `RedbResultMailbox`). `submit` returns only after the task is committed to the lease queue; the task's lease is extended while it waits or runs and is acked only after its outcome is stored in the result mailbox, so a process killed at any point (tested with `SIGKILL` mid-execution and between execution and the mailbox write, on PG 18 and redb) has the task re-run after the lease expires and exactly one result consumable, whichever consumer re-runs it: results are keyed by the queue's identity, not the pool's name, so any number of pools can consume one queue and pools on different queues can share a mailbox. The executor itself can run more than once for one task (at-least-once); make side effects idempotent. In-memory pools and the `WorkerPool` still lose parked work on a crash. |
| No silent loss at shutdown | **Yes, for all three pools** (cr-29): `shutdown_with(ShutdownPolicy)` (default `DrainThenCancel { deadline: 30 s }`) returns a `ShutdownReport` in which every accepted task either completed, was cancelled, or is still running (`unsettled`). The in-memory pools hand each cancelled task to a `CancellationSink` (its mailbox or result slot gets `TaskError::Cancelled`); the durable pool hands it back to its queue (`nack`, requeue), and a restarted pool runs it (tested on PG 18 and redb). A running task cancelled by the native `WorkerPool` reaches the sink without its payload. |
| Lifecycle events | Yes, for `ResourcePool`, `DurableResourcePool`, the native `WorkerPool` and the WASM `WorkerPool` (cr-31, cr-53): every task emits `Submitted`, then `Queued` if it parks, `Started` if it runs, and exactly one terminal event (`Completed`, `Failed`, `Rejected`, `Cancelled`, `Expired` or `Requeued`). A durable task can run more than once, so `DurableResourcePool` emits one terminal event per run: every run but the last ends in `Requeued` (handed back to the queue), and the next run's `Queued`/`Started` follow it. Hooks run on a background thread fed by a bounded queue, so a slow or panicking hook never stalls dispatch; when that queue is full, events are dropped and counted. With no hook registered, emission costs one atomic load. On WASM there are no threads, so hooks run inline on the call that causes the transition (`Queued` under the dispatch lock, so a hook must not call back into the pool), and a panicking hook aborts the module. Tasks removed by `prune_expired` emit no terminal event (issue #57). |
| Result notifications | `StorageMailbox` stores a result first, then notifies off the delivery path with bounded retries; a failing notifier never loses or fails a stored result (cr-34). `IggyNotifier` (feature `iggy`) publishes `mailbox.ready` to Apache Iggy (cr-32). `IggyStreamSink` publishes a task's streamed output as numbered chunks, and `IggyStreamReader::resume` gives a reconnecting client the missing chunks with no gap or duplicate (cr-33). |

## Crates and features

Most applications depend only on the facade, `prometheus_parking_lot`, which
re-exports the rest.

| Crate | What it holds |
| --- | --- |
| `prometheus_parking_lot` | Facade: re-exports, scheduler configuration (`config`), pool builders (`builders`) |
| `prometheus-parking-lot-core` | Runtime-free contracts and types, the dispatcher and semaphore, hooks and audit types, in-memory backends. No async runtime, IO or database dependency. |
| `prometheus-parking-lot-runtime-tokio` | `ResourcePool` and its crash-durable mode `DurableResourcePool`, the native `WorkerPool`, shutdown, cancellation, `TokioSpawner` |
| `prometheus-parking-lot-pg` | Postgres lease queue (`PgLeaseQueue`), the same contract over the pgmq extension (`PgmqLeaseQueue`), result mailbox (`PgResultMailbox`) and a batching audit sink (`PostgresAuditSink`), on raw sqlx, plus migrations. Tested on PostgreSQL 17 and 18. |
| `prometheus-parking-lot-embedded` | Crash-safe redb lease queue and result mailbox for desktop and single-node use (`EmbeddedStore`, `RedbLeaseQueue`, `RedbResultMailbox`; ADR 0004). Needs a tokio runtime. A `DurableResourcePool` runs on it (cr-49). |
| `prometheus-parking-lot-iggy` | Apache Iggy events and notifications (`IggyHook`, `IggyNotifier`; cr-32; `IggyStreamSink` and `IggyStreamReader` for gap-free stream resume; cr-33). Lifecycle events and `mailbox.ready` go to Iggy through a bounded off-path publisher with UUIDv5 message ids (derived from the idempotency key for keyed `mailbox.ready` and stream chunks, from a per-instance nonce and sequence otherwise); Iggy is not mailbox storage or the work queue. Needs a tokio runtime and an Iggy server (tested against 0.9). |
| `prometheus-parking-lot-ffi` | UniFFI (Kotlin, Swift) bindings over the embedded queue and mailbox for Android and iOS; not published. See `bindings/README.md`; `bindings/tauri-desktop` is a Tauri 2 example whose queue survives an app restart. |
| `prometheus-parking-lot-wasm` | `WorkerPool` for `wasm32-unknown-unknown` on the shared dispatcher, with `spawn_local` and JS-timer budgets (cr-38), the same per-dimension admission, skip-ahead and lifecycle events as the native pool (cr-53). No durable dispatch (the browser has no durable backend) and panics abort the module. Built and tested in CI. |

| Facade feature | Enables | Default |
| --- | --- | --- |
| `tokio-runtime` | `ResourcePool`, `DurableResourcePool`, native `WorkerPool`, shutdown, cancellation, `TokioSpawner`, `builders`, `runtime` | yes |
| `postgres` | `infra::queue::{PgLeaseQueue, PgmqLeaseQueue, QueueListener}`, `infra::mailbox::PgResultMailbox`, `infra::PgPool`, `core::PostgresAuditSink` | no |
| `embedded` | `infra::EmbeddedStore`, `infra::queue::RedbLeaseQueue`, `infra::mailbox::RedbResultMailbox` | no |
| `iggy` | `infra::events::{IggyHook, IggyNotifier, IggyOptions, IggyStreamSink, IggyStreamReader}`, `config::IggyBackendConfig` (`"type": "iggy"`) | no |

**Building for `wasm32-unknown-unknown`.** The default `tokio-runtime` feature
pulls in tokio's multi-thread runtime and OS signal handling, which do not build
for that target (`mio` is unsupported). Turn the defaults off; the facade then
exposes the runtime-free API and the wasm `WorkerPool`:

```toml
[dependencies]
prometheus_parking_lot = { version = "0.3", default-features = false }
```

CI runs `cargo check --target wasm32-unknown-unknown -p prometheus_parking_lot
--no-default-features`. The default features are not supported on that target.

The core crate has one feature, `conformance`, which publishes the test suite
every queue and mailbox backend must pass.

## Quick start

```toml
[dependencies]
prometheus_parking_lot = "0.3"
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
```

Then pick a pool:

- Use **`WorkerPool`** when the work is CPU- or GPU-bound, or the result
  cannot be serialized, and the caller waits for the result.
- Use **`ResourcePool`** when the work is async and its result should land in
  a mailbox that a client can read later.

Both examples below are compiled and run as doctests of the facade crate
(`cargo test --doc`) on every CI run.

## Example: `WorkerPool`

Two worker threads and 8 units of capacity. An executor returns
`Result<R, TaskError>`; a failure comes back from `retrieve` as
`SchedulerError::Task`.

```rust
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::WorkerPool;
use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{SchedulerError, TaskError, TaskMetadata, WorkerExecutor};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind, now_ms};

/// Splits text into words. Stands in for an inference call.
#[derive(Clone)]
struct Tokenizer;

#[async_trait]
impl WorkerExecutor<String, Vec<String>> for Tokenizer {
    async fn execute(&self, text: String, _meta: TaskMetadata) -> Result<Vec<String>, TaskError> {
        if text.is_empty() {
            return Err(TaskError::executor("empty input"));
        }
        Ok(text.split_whitespace().map(str::to_owned).collect())
    }
}

fn meta(cpu_units: u32) -> TaskMetadata {
    TaskMetadata {
        id: 0,
        mailbox: None,
        priority: Priority::Normal,
        cost: ResourceCost::new(ResourceKind::Cpu, cpu_units),
        deadline_ms: None,
        created_at_ms: now_ms(),
        idempotency_key: None,
        sequence: 0, // the pool stamps the submission order
    }
}

fn main() -> Result<(), SchedulerError> {
    let pool = WorkerPool::new(
        WorkerPoolConfig::new()
            .with_worker_count(2)
            .with_max_units(8)
            .with_max_queue_depth(16)
            .with_timeout_ms(5_000), // execution budget per task
        Tokenizer,
    )?;

    // Submit, then wait up to five seconds for the result.
    let key = pool.submit("the quick brown fox".to_owned(), meta(2))?;
    let words = pool.retrieve(&key, Duration::from_secs(5))?;
    assert_eq!(words, ["the", "quick", "brown", "fox"]);

    // The executor's error reaches the caller.
    let key = pool.submit(String::new(), meta(1))?;
    let err = pool.retrieve(&key, Duration::from_secs(5)).unwrap_err();
    assert!(matches!(err, SchedulerError::Task(TaskError::Executor { .. })));

    // A task costing more than the pool's 8 units could never start.
    let err = pool.submit("too big".to_owned(), meta(9)).unwrap_err();
    assert!(matches!(err, SchedulerError::CapacityExceeded { .. }));

    println!("{:?}", pool.stats());
    pool.shutdown();
    Ok(())
}
```

From async code, use `submit_async` and `retrieve_async`; they take the same
arguments.

## Example: `ResourcePool` with a mailbox

Four GPU units and three tasks of two units each: two run, the third parks
until one finishes. Each outcome is delivered to the mailbox under the task's
`MailboxKey`, and the caller collects it afterwards, as a client that
reconnects would.

The pool owns its mailbox, so this example wraps the in-memory mailbox in a
shared handle. The gate only makes the example deterministic: it holds the
executors while the example inspects the pool.

```rust
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::core::{
    Mailbox, PoolLimits, ResourcePool, ScheduledTask, SchedulerError, TaskError, TaskExecutor,
    TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::{InMemoryMailbox, InMemoryQueue};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};
use tokio::sync::Semaphore;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct Prompt {
    text: String,
}

/// Upper-cases the prompt once the gate lets it through.
#[derive(Clone)]
struct Shout {
    gate: Arc<Semaphore>,
}

#[async_trait]
impl TaskExecutor<Prompt, String> for Shout {
    async fn execute(&self, job: Prompt, _meta: TaskMetadata) -> Result<String, TaskError> {
        let _pass = self.gate.acquire().await.map_err(TaskError::executor)?;
        Ok(job.text.to_uppercase())
    }
}

/// One in-memory mailbox shared by the pool (which delivers) and the caller
/// (which collects).
#[derive(Clone, Default)]
struct SharedMailbox(Arc<Mutex<InMemoryMailbox<String>>>);

impl Mailbox<String> for SharedMailbox {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<String>,
    ) -> Result<(), SchedulerError> {
        self.0.lock().expect("mailbox lock").deliver(key, status, payload)
    }
}

impl SharedMailbox {
    /// Wait (up to five seconds) for the outcome under `key` and take it.
    async fn collect(&self, key: &MailboxKey) -> (TaskStatus, Option<String>) {
        for _ in 0..500 {
            let outcome = self.0.lock().expect("mailbox lock").consume(key).expect("consume");
            if let Some(outcome) = outcome {
                return outcome;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("no outcome for {key:?}");
    }
}

fn session(id: &str) -> MailboxKey {
    MailboxKey {
        tenant: "acme".into(),
        user_id: Some("user-1".into()),
        session_id: Some(id.into()),
    }
}

fn task(id: u64, gpu_units: u32, key: &MailboxKey, text: &str) -> ScheduledTask<Prompt> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(key.clone()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::GpuVram, gpu_units),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0, // the pool stamps the submission order
        },
        payload: Prompt { text: text.into() },
    }
}

#[tokio::main]
async fn main() -> Result<(), SchedulerError> {
    let gate = Arc::new(Semaphore::new(0));
    let mailbox = SharedMailbox::default();
    let pool = ResourcePool::new(
        PoolLimits {
            max_units: 4,
            max_queue_depth: 16,
            default_timeout: Duration::from_secs(30), // execution budget per task
        },
        InMemoryQueue::new(16),
        mailbox.clone(),
        Shout { gate: Arc::clone(&gate) },
        TokioSpawner::new(tokio::runtime::Handle::current()),
    );

    let keys = [session("a"), session("b"), session("c")];
    let mut statuses = Vec::new();
    for (id, key) in (1..).zip(&keys) {
        statuses.push(pool.submit(task(id, 2, key, "hello")).await?);
    }
    assert_eq!(
        statuses,
        [TaskStatus::Running, TaskStatus::Running, TaskStatus::Queued]
    );
    assert_eq!(
        (pool.active_units(), pool.running_tasks(), pool.queued_tasks()),
        (4, 2, 1)
    );

    // A task costing more than the pool's 4 units is rejected, not parked.
    let err = pool.submit(task(4, 5, &session("d"), "huge")).await.unwrap_err();
    assert!(matches!(err, SchedulerError::CapacityExceeded { .. }));

    // Let the executors finish. The parked task starts when units free up.
    gate.add_permits(keys.len());
    for key in &keys {
        let (status, value) = mailbox.collect(key).await;
        assert_eq!(status, TaskStatus::Completed);
        assert_eq!(value.as_deref(), Some("HELLO"));
    }
    assert_eq!(pool.active_units(), 0);
    Ok(())
}
```

A failed task is delivered as `TaskStatus::Failed(TaskError)` with no payload.
A task with `mailbox: None` runs, but its outcome is not stored.

## Shutdown without losing work

`shutdown_with(policy)` refuses new work, lets accepted work run until the
policy's drain deadline, then cancels what is left, and reports where every
task went. Each cancelled task's mailbox receives `TaskError::Cancelled`, and
the task, with its payload, goes to the pool's `CancellationSink` (by default
one that logs it). `ShutdownPolicy::Drain` never cancels a running task;
`CancelImmediately` drains for zero time. The durable pool needs no sink: it
hands cancelled tasks back to its queue, and the next pool on that queue runs
them.

```rust
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::core::{
    CancellationSink, PoolLimits, RecordingCancellationSink, ResourcePool, ScheduledTask,
    ShutdownPolicy, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::{InMemoryMailbox, InMemoryQueue};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind, now_ms};

/// Sleeps for the number of milliseconds in the payload.
#[derive(Clone)]
struct Sleep;

#[async_trait]
impl TaskExecutor<u64, u64> for Sleep {
    async fn execute(&self, ms: u64, _meta: TaskMetadata) -> Result<u64, TaskError> {
        tokio::time::sleep(Duration::from_millis(ms)).await;
        Ok(ms)
    }
}

fn task(id: u64, ms: u64) -> ScheduledTask<u64> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: None,
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: ms,
    }
}

#[tokio::main]
async fn main() {
    let sink = Arc::new(RecordingCancellationSink::<u64>::new());
    let pool = ResourcePool::new(
        PoolLimits {
            max_units: 1,
            max_queue_depth: 8,
            default_timeout: Duration::from_secs(60),
        },
        InMemoryQueue::new(8),
        InMemoryMailbox::new(),
        Sleep,
        TokioSpawner::new(tokio::runtime::Handle::current()),
    )
    .with_cancellation_sink(Arc::clone(&sink) as Arc<dyn CancellationSink<u64>>);

    // One task runs for a minute; one waits behind it.
    assert_eq!(pool.submit(task(1, 60_000)).await.unwrap(), TaskStatus::Running);
    assert_eq!(pool.submit(task(2, 10)).await.unwrap(), TaskStatus::Queued);

    let report = pool
        .shutdown_with(ShutdownPolicy::DrainThenCancel {
            deadline: Duration::from_millis(100),
        })
        .await;
    // Both were cancelled at the deadline and recorded by the sink, the
    // running one with its payload too.
    assert_eq!((report.cancelled, report.recorded, report.unsettled), (2, 2, 0));
    let mut payloads: Vec<_> = sink.drain().await.into_iter().map(|t| t.payload).collect();
    payloads.sort_unstable();
    assert_eq!(payloads, [Some(10), Some(60_000)]);
}
```

## Configuration

`config::SchedulerConfig::from_json_str` parses and validates a JSON file of
named `pools` (limits and execution budget) and optional named `backends`
with their parameters: `"type": "postgres"` or `"redb"` (durable backends a pool
can run on), `"pgmq"` (opens a `PgmqLeaseQueue` for direct use; no pool runs on
it) and `"iggy"` (event publishing options; feature `iggy`).
`builders::build_pools_from_scheduler_config` builds every pool and opens
every backend; `builders::build_pools` builds in-memory pools from queue and
mailbox factories you supply instead.

The schema is strict: an unknown or misspelled field fails to load, and the
error names it. A pool's optional `max_head_skips` (default 8) bounds how often
a parked task that does not fit may be passed by smaller tasks that do before
the pool reserves capacity for it; `0` keeps strict head-of-line order. A
pool's optional `resource_limits` maps resource kinds (`cpu`, `gpu_vram`,
`io`, `mixed`) to the units each may hold at once, e.g.
`"resource_limits": {"gpu_vram": 24, "cpu": 4}`: a task then starts only when
its cost fits `max_units`, which bounds the total over every kind, and every
listed kind at once (cr-23). Each limit must be greater than zero and at most
`max_units`; a kind not listed is bounded by `max_units` alone, and an empty or
absent map admits by the total, as before. `WorkerPoolConfig` takes the same
field. A pool whose `queue` and `mailbox` are `"in_memory"` parks
tasks and delivers results in memory. A pool whose `queue` and `mailbox` both
name a backend (`{"backend": "local"}`) is built as a `DurableResourcePool` on
it (cr-49): accepted tasks and their results survive a crash. Each backend
drives at most one pool, and a pool with one half in memory is rejected. One
example per backend, each loaded and built by the test suite:
[`in-memory.json`](examples/configs/in-memory.json),
[`postgres.json`](examples/configs/postgres.json),
[`redb.json`](examples/configs/redb.json),
[`pgmq.json`](examples/configs/pgmq.json),
[`iggy.json`](examples/configs/iggy.json).

## Documentation

- [Documentation site](https://prometheus-ags.github.io/prometheus-parking-lot-rs/):
  concepts (parking lots, admission, leases, mailboxes, worker pools, event
  logs) and their current state on `main`, and
  [guides](https://prometheus-ags.github.io/prometheus-parking-lot-rs/docs/guides)
  per deployment: a GPU inference pool, a Tauri local LLM, a multi-tenant
  Postgres service, disconnected clients, shutdown and requeue, Iggy events,
  streaming replay, configuration, and upgrading from 0.2.
- [examples/](examples/): the programs behind the guides, built and run in
  CI, the Postgres and Iggy ones against containers
  (`cargo run --example gpu_inference_pool`; each file's header gives its
  features and setup).
- [docs/API.md](docs/API.md): map of the public API by module and feature.
- [docs/adr/](docs/adr/): design decisions (workspace split, core contracts,
  backend contracts, embedded backend).
- [bindings/README.md](bindings/README.md): Android and iOS bindings and the
  Tauri desktop example.
- [docs/runbooks/postgres18.md](docs/runbooks/postgres18.md): running the
  Postgres backends on PostgreSQL 18.
- [CHANGELOG.md](CHANGELOG.md): what changed, with migration notes.
- [docs/README.md](docs/README.md): index of the rest.
- Rustdoc: on [docs.rs](https://docs.rs/prometheus_parking_lot), or locally with
  `cargo doc --workspace --no-deps --all-features --open`.

## License

MIT ([LICENSE-MIT](LICENSE-MIT)).
