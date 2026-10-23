# prometheus_parking_lot

A resource-aware scheduler for expensive work such as LLM inference. Each task
declares what it costs (for example 4 units of GPU memory). A pool starts the
task when its cost fits in the capacity that is free and nothing is waiting
ahead of it; otherwise it parks the task and starts parked tasks, highest
priority first and in submission order within a priority, as running tasks
release their units. A task that costs more than the pool's total capacity is
rejected at submission instead of blocking the queue forever.

Two pools share one admission core:

- **`ResourcePool`** runs async executors on a tokio runtime and delivers each
  outcome (value or error) to a **mailbox** keyed by tenant, user and session,
  so a client that disconnected can collect it later.
- **`WorkerPool`** runs executors on dedicated OS threads, each with its own
  single-threaded tokio runtime, so CPU- or GPU-bound work never blocks your
  async runtime. Results need not be serializable (a streaming channel is
  fine) and are collected with `retrieve`.

Every execution runs under a budget: the task's deadline or the pool's default
timeout, whichever is nearer. Work still running at its budget is cancelled and
its units are released.

This README describes `main`, which will be released as **0.3.0**. 0.3.0 is
source-breaking for 0.2 users; the [CHANGELOG](CHANGELOG.md) has the
migration notes.

## Status: what holds today and what does not

| Guarantee | Today |
| --- | --- |
| Admitted units never exceed `max_units` | Yes, for both pools: one dispatcher and one unit-weighted semaphore, covered by loom and property tests. |
| Priority, then FIFO | Yes. Letting small tasks skip ahead of a large head task is planned (cr-20). |
| Execution budgets | Yes, for both pools. |
| Multi-resource admission | Costs are already vectors, but pools admit by the sum of units against one `max_units`; per-dimension limits are cr-23. |
| Executor panics | Contained, for `ResourcePool` and the native `WorkerPool` (cr-21): the outcome is `TaskError::ExecutorPanicked`, the task's units are released and parked work starts; a `WorkerPool` worker killed by a panic is replaced. |
| Accepted work survives a crash | **Not through the pools yet.** Both pools park tasks in memory and lose them on restart. The embedded redb `RedbLeaseQueue` and `RedbResultMailbox` (feature `embedded`, cr-28) are crash-safe: every operation is one durably committed transaction. They implement the async `LeaseQueue` and `ResultMailbox` contracts, which no pool uses until durable dispatch (cr-49); until then you call them directly (`examples/embedded_pool.rs`). The current Postgres queue deletes a row when it dequeues it (at-most-once) and bridges async calls with `block_on`, which panics inside a tokio runtime; leasing Postgres backends replace it (cr-26, cr-27). |

## Crates and features

Most applications depend only on the facade, `prometheus_parking_lot`, which
re-exports the rest.

| Crate | What it holds |
| --- | --- |
| `prometheus_parking_lot` | Facade: re-exports, scheduler configuration (`config`), pool builders (`builders`) |
| `prometheus-parking-lot-core` | Runtime-free contracts and types, the dispatcher and semaphore, hooks and audit types, in-memory backends. No async runtime, IO or database dependency. |
| `prometheus-parking-lot-runtime-tokio` | `ResourcePool`, the native `WorkerPool`, shutdown, cancellation, `TokioSpawner` |
| `prometheus-parking-lot-pg` | Postgres queue, mailbox and audit sink (SeaORM) and migrations; replaced by cr-26 and cr-27 |
| `prometheus-parking-lot-embedded` | Crash-safe redb lease queue and result mailbox for desktop and single-node use (`EmbeddedStore`, `RedbLeaseQueue`, `RedbResultMailbox`; ADR 0004). Needs a tokio runtime. Not usable by the pools until cr-49. |
| `prometheus-parking-lot-wasm` | `WorkerPool` for `wasm32` (async tasks instead of threads); building it for `wasm32-unknown-unknown` is cr-38 |

| Facade feature | Enables | Default |
| --- | --- | --- |
| `tokio-runtime` | `ResourcePool`, native `WorkerPool`, shutdown, cancellation, `TokioSpawner`, `builders`, `runtime` | yes |
| `postgres` | `infra::queue::PostgresQueue`, `infra::mailbox::PostgresMailbox`, `core::PostgresAuditSink`, SeaORM entities | no |
| `embedded` | `infra::EmbeddedStore`, `infra::queue::RedbLeaseQueue`, `infra::mailbox::RedbResultMailbox` | no |

The core crate has one feature, `conformance`, which publishes the test suite
every queue and mailbox backend must pass.

## Quick start

0.3.0 is not on crates.io yet. Until it is, depend on the repository:

```toml
[dependencies]
prometheus_parking_lot = { git = "https://github.com/Prometheus-AGS/prometheus-parking-lot-rs" }
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

## Configuration

`config::SchedulerConfig::from_json_str` parses and validates a JSON file of
named `pools` (limits and execution budget) and optional named durable
`backends` (Postgres or redb, with their parameters).
`builders::build_pools_from_scheduler_config` builds every pool and opens
every backend; `builders::build_pools` builds pools from queue and mailbox
factories you supply instead.

The schema is strict: an unknown or misspelled field fails to load, and the
error names it. Pools park tasks and deliver results in memory; a pool that
names a durable backend for its `queue` or `mailbox` is rejected until durable
dispatch lands, and the opened backends are used directly through
`LeaseQueue` and `ResultMailbox`. One example per backend, each loaded and
built by the test suite:
[`in-memory.json`](examples/configs/in-memory.json),
[`postgres.json`](examples/configs/postgres.json),
[`redb.json`](examples/configs/redb.json).

## Documentation

- [docs/API.md](docs/API.md): map of the public API by module and feature.
- [docs/adr/](docs/adr/): design decisions (workspace split, core contracts,
  backend contracts).
- [CHANGELOG.md](CHANGELOG.md): what changed, with migration notes.
- [docs/README.md](docs/README.md): index of the rest.
- Rustdoc: `cargo doc --workspace --no-deps --all-features --open`.

## License

MIT ([LICENSE-MIT](LICENSE-MIT)).
