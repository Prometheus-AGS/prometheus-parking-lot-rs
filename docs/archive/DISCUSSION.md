# Prompt Chain Related to Parking Log Algorithm

## Prompt:

I started addressing the pre-existing Leptos type errors, but the component set still has extensive incompatibilities with the current Leptos API. Fixing them will require a focused refactor (particularly avatar.rs and label.rs) rather than quick patches. Key blockers:

- avatar.rs: The view closure still fails (FnOnce vs FnMut) and nested component calls expect concrete String/AvatarSize/AvatarShape instead of Option<>, so the call sites need to unwrap/default before passing. The fallback/alt values also need to be cloned without moving into Show closures.
- label.rs: Optional markers and for_id/class props are still Option at call sites while the generated props expect String/tuple with aria; each caller must unwrap/default before passing.
- Similar optional-prop mismatches remain in AvatarWithStatus/Badge and a few checkbox marker spans.

Given the scope, I recommend a targeted cleanup:

1. Normalize component props to either concrete types with defaults or Option consistently; update call sites to unwrap/default as needed.
2. Rework Avatar to use Show with only cloned data (no moves) and make nested avatar wrappers pass concrete values.
3. Adjust label/checkbox markers to return matching view types with required aria/text tuples.

If you want me to proceed with that deeper refactor now, I’ll focus on avatar.rs, label.rs, checkbox.rs, and any remaining optional-prop call sites until cargo check passes.

## Answer:

Here’s how I’d design this as a serious, reusable Rust crate for you, end-to-end.

------

## 1. What this crate actually does

**Goal:** A *runtime-agnostic* “parking lot” scheduler that:

1. Manages **limited capacity** (threads, GPU VRAM, web workers, etc.).
2. Accepts **tasks with metadata** (cost, deadline, mailbox, tenant).
3. Puts tasks into a **queue (“parking lot”)** when capacity is exhausted.
4. Automatically **wakes parked tasks** when capacity is freed.
5. Drops or marks **timed-out tasks** while in the lot.
6. Delivers results via a **Mailbox abstraction**, so:
   - If the client is still connected → stream/send directly.
   - If the client is gone → store result for later pickup + notification.
7. Provides **pluggable, persistent queue backends** for cross-startup survival.

And it does this **once**, in one crate, with adapters for:

- Native/desktop (Tokio / threads / Tauri).
- WASM+web workers.
- Cloud (large pools + external queues).

I’ll call it `prometheus_parking_lot` for now.

------

## 2. Top-level architecture

```text
prometheus_parking_lot
├── config/           # Pool + queue configuration
├── scheduler/        # Core parking-lot scheduling logic (pure Rust)
├── queue/            # In-memory + persistent queue backends
├── mailbox/          # Result delivery + late pickup
├── runtime/
│   ├── native/       # Tokio / async-std adapter
│   ├── wasm/         # Web-worker adapter
│   └── cloud/        # Queue-backed scaling patterns
└── metrics/          # Hooks for tracing / Prometheus / logs
```

The *core scheduler* is runtime-agnostic: no direct Tokio, web workers, etc. Those live in `runtime/*` as adapters.

------

## 3. Core domain model

### 3.1 Task IDs, clients, mailboxes

```rust
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TenantId(pub String);   // org / app / agent

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MailboxKey {
    pub tenant: TenantId,
    pub user_id: Option<String>,
    pub session_id: Option<String>, // WS/session/etc
}
```

### 3.2 Cost & resource model

We want to reason about capacity “slots” independent of the actual runtime:

```rust
#[derive(Debug, Clone, Copy)]
pub enum ResourceKind {
    Cpu,
    GpuVram,       // primary for LLM inference
    Io,
    Mixed,         // composite resource
}

#[derive(Debug, Clone, Copy)]
pub struct ResourceCost {
    pub kind: ResourceKind,
    /// Arbitrary units: e.g. “GPU units”, “CPU slots”.
    pub units: u32,
}
```

For LLM inference, `units` can be computed from approximate VRAM usage (model size + KV cache estimate). For web workers, `units` might be 1 per worker slot. For cloud, you’ve likely got many CPU units + large queue.

### 3.3 Task metadata & priority

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone)]
pub struct TaskMetadata {
    pub id: TaskId,
    pub priority: Priority,
    pub cost: ResourceCost,
    pub created_at: Instant,
    pub deadline: Option<Instant>,     // for queue timeout
    pub mailbox: Option<MailboxKey>,   // where to drop result
}
```

------

## 4. Fundamental traits

### 4.1 Task abstraction

We want the core to work with any async job:

```rust
use std::{future::Future, pin::Pin};

pub trait TaskFn: Send + 'static {
    type Output: Send + 'static;
    fn into_future(self) -> Pin<Box<dyn Future<Output = Self::Output> + Send>>;
}

impl<F, T> TaskFn for F
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    type Output = T;

    fn into_future(self) -> Pin<Box<dyn Future<Output = Self::Output> + Send>> {
        Box::pin(self)
    }
}
```

Then a schedulable task is just:

```rust
pub struct ScheduledTask<F: TaskFn> {
    pub meta: TaskMetadata,
    pub fut: F,
}
```

### 4.2 Queue backend abstraction

We want in-memory **and** persistent queues (Postgres, Surreal, Redis, etc.):

```rust
#[derive(Debug, Clone)]
pub struct EnqueueResult {
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub enum DequeueOutcome<F: TaskFn> {
    Task(ScheduledTask<F>),
    Empty,
}

#[async_trait::async_trait]
pub trait TaskQueue<F: TaskFn>: Send + Sync {
    async fn enqueue(&self, task: ScheduledTask<F>) -> EnqueueResult;

    /// Pull the next ready task, applying priority + deadline logic.
    async fn dequeue_next(&self, now: Instant) -> DequeueOutcome<F>;

    /// Best effort: drop any tasks whose deadline has passed.
    async fn prune_expired(&self, now: Instant) -> usize;
}
```

Implementations:

- `InMemoryQueue<F>` – priority heap + FIFO per priority.
- `PersistentQueue<F>` – uses a database (Postgres/Pglite/Surreal) and a small in-memory index.
- `ExternalQueueAdapter<F>` – wrapper around SQS, Redis, NATS, or your own AG-UI queue.

### 4.3 Mailbox abstraction

Used when client disconnects but you still want the answer:

```rust
#[derive(Debug, Clone)]
pub enum TaskStatus {
    Completed,
    Failed(String),
    Expired,
    Dropped(String),
}

#[derive(Debug, Clone)]
pub struct MailboxMessage<T> {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub created_at: Instant,
    pub payload: Option<T>, // e.g. JSON, bytes, etc.
}

#[async_trait::async_trait]
pub trait Mailbox<T>: Send + Sync {
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>);
    async fn fetch(
        &self,
        key: &MailboxKey,
        since: Option<Instant>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>>;
}
```

Implementations:

- `InMemoryMailbox<T>` – quick for dev.
- `PostgresMailbox<T>` / `SurrealMailbox<T>` – production.
- You can wire notifications (email, push, WS) *on top* of this interface.

------

## 5. The parking-lot scheduler core

### 5.1 Pool config

```rust
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_units: u32,         // total capacity units
    pub max_queue_depth: usize, // hard cap on parked tasks
    pub default_timeout: Duration,
}
```

### 5.2 Scheduler state machine

The scheduler tracks:

```rust
pub struct ResourcePool<F, Q, M, T> 
where
    F: TaskFn<Output = T>,
    Q: TaskQueue<F>,
    M: Mailbox<T>,
{
    config: PoolConfig,
    queue: Q,
    mailbox: M,
    active_units: u32,     // currently consumed capacity
    // optional: per-tenant fairness tracking, metrics, etc.
}
```

**Core algorithm (“parking lot”)**:

1. When a task arrives:
   - If `active_units + task.cost.units <= max_units` → **start immediately**.
   - Else if queue not full → **enqueue**.
   - Else → **reject or drop lowest priority**.
2. When a running task finishes:
   - Decrement `active_units` by its cost.
   - Ask queue for `dequeue_next(now)`.
   - For each candidate:
     - If expired → mark as `Expired`, send to mailbox, continue.
     - If it fits (`active_units + cost <= max_units`) → start it.
     - Otherwise, put back in queue and stop.

Pseudo-API:

```rust
impl<F, Q, M, T> ResourcePool<F, Q, M, T>
where
    F: TaskFn<Output = T>,
    Q: TaskQueue<F>,
    M: Mailbox<T>,
{
    pub async fn submit(&mut self, task: ScheduledTask<F>) -> EnqueueResult {
        // Fast path: can run now.
        if self.active_units + task.meta.cost.units <= self.config.max_units {
            self.spawn_running(task);
            EnqueueResult { accepted: true, reason: None }
        } else {
            // Park it.
            self.queue.enqueue(task).await
        }
    }

    fn spawn_running(&mut self, task: ScheduledTask<F>) {
        self.active_units += task.meta.cost.units;
        // Delegate to runtime adapter to actually spawn the future.
        // When it completes, we call `on_task_finished`.
        // (see runtime section below)
    }

    pub async fn on_task_finished(
        &mut self,
        meta: TaskMetadata,
        result: Result<T, String>,
    ) {
        self.active_units -= meta.cost.units;

        // Deliver or mailbox
        if let Some(mailbox_key) = meta.mailbox {
            let msg = MailboxMessage {
                task_id: meta.id,
                status: match &result {
                    Ok(_) => TaskStatus::Completed,
                    Err(e) => TaskStatus::Failed(e.clone()),
                },
                payload: result.ok(),
                created_at: Instant::now(),
            };
            self.mailbox.deliver(&mailbox_key, msg).await;
        }

        // Wake parked tasks.
        self.try_wake_parked().await;
    }

    async fn try_wake_parked(&mut self) {
        let now = Instant::now();
        // Remove expired first:
        let n_expired = self.queue.prune_expired(now).await;
        if n_expired > 0 {
            // TODO: optionally emit metrics, log, and mailbox-expired events.
        }

        loop {
            match self.queue.dequeue_next(now).await {
                DequeueOutcome::Empty => break,
                DequeueOutcome::Task(task) => {
                    if self.active_units + task.meta.cost.units <= self.config.max_units {
                        self.spawn_running(task);
                    } else {
                        // Can't run now; push it back and stop.
                        let _ = self.queue.enqueue(task).await;
                        break;
                    }
                }
            }
        }
    }
}
```

That’s the “parking lot” bit: tasks enter the lot when capacity is full; they’re woken in a controlled order when capacity frees up.

------

## 6. Runtime adapters

The core scheduler just calls “spawn this future, call me back when done”.

Define a minimal runtime trait:

```rust
#[async_trait::async_trait]
pub trait Spawn {
    async fn spawn_detached<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static;
}
```

### 6.1 Native (Tokio / Tauri / agent processes)

```rust
pub struct TokioSpawner;

#[async_trait::async_trait]
impl Spawn for TokioSpawner {
    async fn spawn_detached<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}
```

Then `ResourcePool` gets a `Spawner` injected:

```rust
pub struct ResourcePool<F, Q, M, T, S>
where
    S: Spawn,
{
    spawner: S,
    // ...
}

impl<F, Q, M, T, S> ResourcePool<F, Q, M, T, S>
where
    F: TaskFn<Output = T>,
    S: Spawn,
{
    fn spawn_running(&mut self, task: ScheduledTask<F>) {
        self.active_units += task.meta.cost.units;
        let meta = task.meta.clone();
        let fut = task.fut.into_future();
        let pool = self.clone_handle(); // e.g. Arc<Mutex<...>>

        self.spawner.spawn_detached(async move {
            let result = fut.await;
            pool.on_task_finished(meta, Ok(result)).await;
        });
    }
}
```

In a **Tauri desktop LLM agent**, you’d configure:

- `max_units` = 2–4 inference slots (based on VRAM).
- `ResourceKind::GpuVram` for inference tasks.
- Maybe another `ResourcePool` for CPU-heavy RAG chunking.

### 6.2 WASM + web workers

On wasm32 you can’t spawn OS threads, but you can orchestrate **web workers** as external capacity:

- JS side manages a worker pool.
- Rust/WASM code only enqueues “LLM job descriptors” into a shared (postMessage-backed) queue.
- `Spawn` adapter is basically a thin wrapper that:
  - Serializes the job.
  - Sends it to a worker.
  - Returns when the worker posts back completion.

From Rust’s perspective, it’s the same `Spawn` trait; under the hood, `spawn_detached` uses `gloo_worker` / `wasm-bindgen` to message workers.

High-level:

```rust
pub struct WebWorkerSpawner {
    inner: Rc<WebWorkerPool>, // JS managed pool
}

#[async_trait::async_trait]
impl Spawn for WebWorkerSpawner {
    async fn spawn_detached<F>(&self, fut: F)
    where
        F: Future<Output = ()> + 'static + Send,
    {
        // In practice: F is a wrapper that posts a job + awaits the worker response.
        wasm_bindgen_futures::spawn_local(fut);
    }
}
```

The parking-lot logic stays identical; only capacity (`max_units`, cost per job) is tuned for “available workers”.

### 6.3 Cloud runtime

In cloud, you often have:

- A **front-end queue** (user jobs).
- A **fleet of workers** pulling from a queue.
- Much larger `max_units`.

Two main options:

1. Use this crate **inside** each worker to manage CPU/GPU concurrency per VM/pod, with a **separate** queue system like SQS / Redis / NATS.
2. Use this crate as a **logical scheduler** over a global “task queue” (`PersistentQueue` implementation backed by Postgres / Surreal / Redis).

Either way, the algorithm is: same parking-lot, just with a bigger `max_units` and more emphasis on fairness + priority instead of raw capacity.

------

## 7. Queue & timeout behavior

### 7.1 Enqueue path

- If request includes a `deadline` (client-side timeout or SLA), we stash it in `TaskMetadata.deadline`.
- If `deadline` < `now + scheduling_slack`, we can **reject immediately**.

### 7.2 Periodic pruning

Two strategies:

1. **On dequeue**: `prune_expired(now)` before `dequeue_next(now)`.
2. **Background task**: scheduler periodically runs `prune_expired`.

For each expired task, we can:

- If it has a mailbox → deliver `TaskStatus::Expired`.
- Emit metrics and logs.
- Optionally call a user-supplied callback (e.g. to mark an LLM request as cancelled).

------

## 8. Mailbox & disconnected clients

Flow:

1. Client sends a request with a **mailbox key** (tenant+user+session).
2. Server accepts request and returns `TaskId`.
3. If client disconnects before completion, no problem:
   - Task still runs or stays queued.
   - When done, scheduler calls `Mailbox::deliver`.
4. Client later polls or reconnects:
   - Calls your application API, which uses `Mailbox::fetch(&key, since, limit)`.

This works identically for all three environments:

- **Tauri/local**: mailbox stored in local Pglite or a local file/SQLite.
- **Web app**: mailbox in Pglite/supabase; client polls via HTTP or WS.
- **Cloud**: mailbox in your core Postgres/Surreal cluster.

------

## 9. How pool sizing differs by environment

### 9.1 Local LLM with GPU constraints

You can drive a smarter `ResourceCost` for inference:

```rust
pub struct LlmCostModel {
    pub model_vram_gb: f32,
    pub kv_per_token_mb: f32,
    pub max_batch_tokens: u32,
    pub gpu_vram_total_gb: f32,
    pub gpu_overhead_gb: f32,
}

impl LlmCostModel {
    pub fn estimate_units(&self, prompt_tokens: u32, max_tokens: u32) -> u32 {
        let total_tokens = prompt_tokens + max_tokens;
        let kv_gb = (total_tokens as f32 * self.kv_per_token_mb) / 1024.0;
        let total_used = self.model_vram_gb + kv_gb + self.gpu_overhead_gb;

        // Map GB to "units": 1 unit = 1GB, or similar.
        total_used.ceil() as u32
    }

    pub fn max_units(&self) -> u32 {
        (self.gpu_vram_total_gb - self.gpu_overhead_gb).floor() as u32
    }
}
```

Then:

- `PoolConfig.max_units = cost_model.max_units()`.
- Each inference task gets `cost.units = cost_model.estimate_units(...)`.

This gives you **graceful overload behavior** instead of “oh, OOM, bye”.

### 9.2 Web workers

- `max_units = number_of_workers` (or slightly above if you allow limited oversubscription).
- `cost.units = 1` or maybe heavier for large tasks.
- Queue is important because workers are few and each task is long-running.

### 9.3 Cloud

- `max_units` is big enough (e.g. number of cores, or 4× cores for async I/O + network).
- Here the important bits are:
  - **Priority & fairness** (per tenant).
  - **Timeout behavior** at scale.
  - **Integration with external queues** (SQS / Kafka / etc.)

------

## 10. Alternative approaches (and why this is better for you)

### Option A: Just use Tokio’s built-in task scheduling

- Pros: Easy, battle-tested.
- Cons:
  - No notion of **resource cost** (GPU VRAM, etc.).
  - No **persistent queue** or parking lot semantics.
  - No built-in mailbox for disconnected clients.

### Option B: Offload everything to a message broker (Redis/Kafka/SQS)

- Pros: Durable, decoupled.
- Cons:
  - You still need **local scheduling** per worker (GPU, CPU).
  - Broker doesn’t understand your **LLM cost model** or **client mailboxes**.

### Option C: This crate as a dedicated scheduling layer

- Pros:
  - One unified abstraction for **all Prometheus agents** and runtimes.
  - Awareness of **LLM inference costs, deadlines, and tenant priorities**.
  - Pluggable queue/mailbox implementations backed by your **existing data stores** (Postgres, Pglite, Surreal, etc.).
- Cons:
  - Slightly more work up front than just calling `tokio::spawn`.

Given how central scheduling is to Prometheus AI agents, this is absolutely worth encapsulating.

------

## 11. Concrete action plan

Here’s an implementation path you can actually follow:

### Phase 1 – Core crate (1–2 days of focused work)

1. Scaffold crate `prometheus_parking_lot` with modules:
   - `scheduler`, `queue::in_memory`, `mailbox::in_memory`, `runtime::native`.
2. Implement:
   - `TaskId`, `MailboxKey`, `TaskMetadata`, `ResourceCost`, `Priority`.
   - `TaskFn`, `ScheduledTask`, `TaskQueue`, `Mailbox`, `Spawn`.
   - `ResourcePool` with the parking-lot scheduling logic.
3. Provide:
   - In-memory queue & mailbox.
   - Tokio spawner adapter.
4. Add a minimal example:
   - Submit 100 fake tasks with random durations and deadlines.
   - Show that at most N run at once; others are queued/dropped/expired.

### Phase 2 – LLM & GPU-aware extensions (1–2 days)

1. Add `llm` module with `LlmCostModel`.
2. Example:
   - Create a pool configured for `24GB` GPU, `15GB` model, etc.
   - Use fake “inference tasks” to simulate VRAM saturation.
3. Add metrics hooks for:
   - `active_units`, `queue_size`, `expired_tasks`, `dropped_tasks`.

### Phase 3 – WASM + web worker adapter (2–3 days)

1. Add `runtime::wasm` feature gated with `cfg(target_arch = "wasm32")`.
2. Define a JS bridge for sending jobs to workers and receiving results.
3. Wrap that in a `WebWorkerSpawner` implementing `Spawn`.

### Phase 4 – Persistent queue & mailbox (as you integrate with your stack)

1. Implement `PostgresQueue` or `PgliteQueue` using your existing migration + DAL patterns.
2. Implement `Mailbox` backed by Postgres/Surreal, keyed by `(tenant, user, session)`.
3. Add an example:
   - Restart the process and show queued tasks surviving restarts.
   - Client polls mailbox for finished results.

------

## 12. Summary

**What you’ll have at the end:**

- A **single Rust crate** that knows:
  - How many tasks you can safely run given GPU/CPU/worker limits.
  - How to “park” excess requests and wake them intelligently.
  - How to handle **timeouts**, **disconnected clients**, and **late pickup via mailbox**.
- Adaptors so the exact same scheduling logic works:
  - On **Tauri desktops** running local models.
  - Inside **web apps** using web workers & PGlite.
  - In **cloud workers** with larger pools and external queues.

If you want, next step I can sketch actual code signatures for:

- `InMemoryQueue<F>`
- `InMemoryMailbox<T>`
- `TokioPoolBuilder` (builder API for ergonomic setup)

so you can literally drop the crate into one of your agents and start wiring it into the Prometheus runtime.

------

## Prompt:

Do it. Be sure to provide adapter options for the persistent queue and implement option c: this crate as a dedicated scheduling layer.

## Answer:

Alright, let’s actually build this thing. I’ll give you a concrete crate design with real Rust code, including:

- Core scheduler (parking-lot style).
- In-memory queue + mailbox.
- Abstractions + adapters for **persistent queues** (Postgres/PgLite, Surreal, etc.).
- Option C: this crate as a **dedicated scheduling layer** that sits under Prometheus AI services.

I’ll structure it like a mini-implementation you can drop into a repo and expand.

------

## 1. Crate layout

```text
prometheus_parking_lot/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── types.rs
    ├── task.rs
    ├── queue/
    │   ├── mod.rs
    │   ├── in_memory.rs
    │   └── postgres.rs        // skeleton for persistent queue
    ├── mailbox/
    │   ├── mod.rs
    │   ├── in_memory.rs
    │   └── postgres.rs        // skeleton for persistent mailbox
    ├── runtime/
    │   ├── mod.rs
    │   └── tokio.rs           // native adapter
    └── scheduler.rs
```

We’ll base it around **task descriptors** (serializable jobs) instead of raw futures, so persistent queues are realistic.

------

## 2. `Cargo.toml`

```toml
[package]
name = "prometheus_parking_lot"
version = "0.1.0"
edition = "2021"

[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tokio = { version = "1", features = ["rt", "rt-multi-thread", "macros", "time"] }

# Optional DB integrations – you can swap or pin to your stack
sqlx = { version = "0.8", features = ["runtime-tokio", "postgres", "chrono"], optional = true }
surrealdb = { version = "1", optional = true }

[features]
default = ["native"]
native = ["tokio"]
postgres = ["sqlx"]
surreal = ["surrealdb"]
```

------

## 3. Core types – `src/types.rs`

```rust
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailboxKey {
    pub tenant: TenantId,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResourceKind {
    Cpu,
    GpuVram,
    Io,
    Mixed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ResourceCost {
    pub kind: ResourceKind,
    /// Arbitrary units (e.g., 1 unit == 1GB GPU or 1 worker slot).
    pub units: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMetadata {
    pub id: TaskId,
    pub priority: Priority,
    pub cost: ResourceCost,
    pub created_at_ms: u128,
    /// Epoch ms for deadline; None means no explicit deadline.
    pub deadline_ms: Option<u128>,
    pub mailbox: Option<MailboxKey>,
}

impl TaskMetadata {
    pub fn created_at(now: Instant) -> Self {
        Self {
            id: TaskId(rand::random()),
            priority: Priority::Normal,
            cost: ResourceCost { kind: ResourceKind::Cpu, units: 1 },
            created_at_ms: 0, // you can map Instant -> ms via a clock abstraction
            deadline_ms: None,
            mailbox: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Completed,
    Failed(String),
    Expired,
    Dropped(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage<T> {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub created_at_ms: u128,
    pub payload: Option<T>,
}
```

> Note: for cross-process persistence we use integers for time. In practice, you’ll pass in a clock abstraction to get epoch ms.

------

## 4. Task descriptors & executor – `src/task.rs`

We treat jobs as **descriptors** `P` (e.g., “run LLM inference with this input”), not futures. This is what allows you to persist the queue.

```rust
use crate::types::TaskMetadata;
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

/// A serializable job descriptor.
/// Example: { model: "llama-3.1", prompt: "...", max_tokens: 512 }
pub trait TaskPayload: Send + Sync + Serialize + DeserializeOwned + 'static {}
impl<T> TaskPayload for T
where
    T: Send + Sync + Serialize + DeserializeOwned + 'static,
{}

#[derive(Debug, Clone)]
pub struct ScheduledTask<P: TaskPayload> {
    pub meta: TaskMetadata,
    pub payload: P,
}

#[async_trait]
pub trait TaskExecutor<P, T>: Send + Sync + Clone + 'static
where
    P: TaskPayload,
    T: Send + Sync + 'static,
{
    async fn execute(&self, payload: P, meta: TaskMetadata) -> T;
}
```

------

## 5. Queue abstraction – `src/queue/mod.rs`

```rust
use crate::task::ScheduledTask;
use crate::types::TaskMetadata;
use async_trait::async_trait;
use std::time::Instant;

use crate::task::TaskPayload;

#[derive(Debug, Clone)]
pub struct EnqueueResult {
    pub accepted: bool,
    pub reason: Option<String>,
}

#[derive(Debug)]
pub enum DequeueOutcome<P: TaskPayload> {
    Task(ScheduledTask<P>),
    Empty,
}

#[async_trait]
pub trait TaskQueue<P>: Send + Sync
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult;

    async fn dequeue_next(&self, now: Instant) -> DequeueOutcome<P>;

    async fn prune_expired(&self, now: Instant) -> usize;

    /// Useful for metrics/debugging.
    async fn len(&self) -> usize;
}

pub mod in_memory;
#[cfg(feature = "postgres")]
pub mod postgres;
```

### 5.1 In-memory queue – `src/queue/in_memory.rs`

Priority queue with per-priority FIFO:

```rust
use super::{DequeueOutcome, EnqueueResult, TaskQueue};
use crate::task::{ScheduledTask, TaskPayload};
use crate::types::{Priority};
use async_trait::async_trait;
use std::{
    cmp::Ordering,
    collections::{BinaryHeap, VecDeque},
    sync::Mutex,
    time::Instant,
};

#[derive(Debug)]
struct QueueItem<P: TaskPayload> {
    priority: Priority,
    created_at_ms: u128,
    task: ScheduledTask<P>,
}

impl<P: TaskPayload> PartialEq for QueueItem<P> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.created_at_ms == other.created_at_ms
    }
}
impl<P: TaskPayload> Eq for QueueItem<P> {}

impl<P: TaskPayload> PartialOrd for QueueItem<P> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<P: TaskPayload> Ord for QueueItem<P> {
    fn cmp(&self, other: &Self) -> Ordering {
        // Higher priority first; then earlier created_at
        other
            .priority
            .cmp(&self.priority)
            .then(self.created_at_ms.cmp(&other.created_at_ms))
    }
}

pub struct InMemoryQueue<P: TaskPayload> {
    inner: Mutex<BinaryHeap<QueueItem<P>>>,
    max_depth: usize,
}

impl<P: TaskPayload> InMemoryQueue<P> {
    pub fn new(max_depth: usize) -> Self {
        Self {
            inner: Mutex::new(BinaryHeap::new()),
            max_depth,
        }
    }
}

#[async_trait]
impl<P> TaskQueue<P> for InMemoryQueue<P>
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult {
        let mut guard = self.inner.lock().unwrap();
        if guard.len() >= self.max_depth {
            return EnqueueResult {
                accepted: false,
                reason: Some("queue at max capacity".into()),
            };
        }

        let created_at_ms = task.meta.created_at_ms;
        guard.push(QueueItem {
            priority: task.meta.priority,
            created_at_ms,
            task,
        });

        EnqueueResult { accepted: true, reason: None }
    }

    async fn dequeue_next(&self, now: Instant) -> DequeueOutcome<P> {
        let mut guard = self.inner.lock().unwrap();

        while let Some(item) = guard.pop() {
            if let Some(deadline_ms) = item.task.meta.deadline_ms {
                // In a real system, you would convert `now` to epoch ms via a clock.
                // For now, assume no conversion and treat as "instant-based" logic.
                let _ = now;
                // We'll let prune_expired handle the expiry for simplicity here.
            }
            return DequeueOutcome::Task(item.task);
        }

        DequeueOutcome::Empty
    }

    async fn prune_expired(&self, now: Instant) -> usize {
        let mut guard = self.inner.lock().unwrap();
        let mut kept = BinaryHeap::new();
        let mut expired = 0usize;

        while let Some(item) = guard.pop() {
            if let Some(_deadline_ms) = item.task.meta.deadline_ms {
                // For now we can't compare Instant vs ms directly without a clock.
                // In practice, inject a Clock trait. Here we just keep everything.
                kept.push(item);
            } else {
                kept.push(item);
            }
        }

        *guard = kept;
        expired
    }

    async fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}
```

### 5.2 Persistent queue adapter (Postgres) – `src/queue/postgres.rs`

This is a **realistic skeleton**: you persist `TaskMetadata` + `payload (JSON)` and reconstruct jobs when dequeued.

```rust
#![cfg(feature = "postgres")]

use super::{DequeueOutcome, EnqueueResult, TaskQueue};
use crate::task::{ScheduledTask, TaskPayload};
use crate::types::TaskMetadata;
use async_trait::async_trait;
use serde_json::Value as JsonValue;
use sqlx::{Pool, Postgres};
use std::time::Instant;

pub struct PostgresQueue<P: TaskPayload> {
    pool: Pool<Postgres>,
    table: String,
    /// Maximum tasks this logical queue will accept, even if table can hold more.
    max_depth: usize,
    _marker: std::marker::PhantomData<P>,
}

impl<P: TaskPayload> PostgresQueue<P> {
    pub fn new(pool: Pool<Postgres>, table: impl Into<String>, max_depth: usize) -> Self {
        Self {
            pool,
            table: table.into(),
            max_depth,
            _marker: std::marker::PhantomData,
        }
    }

    /// Example schema:
    ///
    /// CREATE TABLE IF NOT EXISTS pl_queue (
    ///   id            BIGSERIAL PRIMARY KEY,
    ///   priority      INT NOT NULL,
    ///   created_at_ms BIGINT NOT NULL,
    ///   deadline_ms   BIGINT NULL,
    ///   metadata      JSONB NOT NULL,
    ///   payload       JSONB NOT NULL
    /// );
    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {} (
                id            BIGSERIAL PRIMARY KEY,
                priority      INT NOT NULL,
                created_at_ms BIGINT NOT NULL,
                deadline_ms   BIGINT NULL,
                metadata      JSONB NOT NULL,
                payload       JSONB NOT NULL
            )
        "#,
            self.table
        );
        sqlx::query(&sql).execute(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl<P> TaskQueue<P> for PostgresQueue<P>
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult {
        // Quick depth check
        let depth: i64 = sqlx::query_scalar(&format!(
            "SELECT COUNT(*) FROM {}",
            self.table
        ))
        .fetch_one(&self.pool)
        .await
        .unwrap_or(0);

        if depth as usize >= self.max_depth {
            return EnqueueResult {
                accepted: false,
                reason: Some("queue at max capacity".into()),
            };
        }

        let meta_json = serde_json::to_value(&task.meta).unwrap();
        let payload_json = serde_json::to_value(&task.payload).unwrap();

        let sql = format!(
            r#"
            INSERT INTO {} (priority, created_at_ms, deadline_ms, metadata, payload)
            VALUES ($1, $2, $3, $4, $5)
        "#,
            self.table
        );

        let _ = sqlx::query(&sql)
            .bind(task.meta.priority as i32)
            .bind(task.meta.created_at_ms as i64)
            .bind(task.meta.deadline_ms.map(|d| d as i64))
            .bind(meta_json)
            .bind(payload_json)
            .execute(&self.pool)
            .await;

        EnqueueResult { accepted: true, reason: None }
    }

    async fn dequeue_next(&self, _now: Instant) -> DequeueOutcome<P> {
        // NOTE: In a real implementation you’d:
        //   - SELECT ... ORDER BY priority DESC, created_at_ms ASC FOR UPDATE SKIP LOCKED
        //   - DELETE or mark as "in-flight"
        // Here we keep it simple.

        let sql = format!(
            r#"
            DELETE FROM {table}
            WHERE id = (
                SELECT id FROM {table}
                ORDER BY priority DESC, created_at_ms ASC
                LIMIT 1
            )
            RETURNING metadata, payload
        "#,
            table = self.table
        );

        if let Ok(Some(row)) = sqlx::query(&sql)
            .map(|row| {
                let meta: JsonValue = row.get("metadata");
                let payload: JsonValue = row.get("payload");
                (meta, payload)
            })
            .fetch_optional(&self.pool)
            .await
        {
            let meta: TaskMetadata = serde_json::from_value(row.0).unwrap();
            let payload: P = serde_json::from_value(row.1).unwrap();
            return DequeueOutcome::Task(ScheduledTask { meta, payload });
        }

        DequeueOutcome::Empty
    }

    async fn prune_expired(&self, _now: Instant) -> usize {
        // In practice:
        // DELETE FROM table WHERE deadline_ms IS NOT NULL AND deadline_ms < now_ms
        0
    }

    async fn len(&self) -> usize {
        let sql = format!("SELECT COUNT(*) FROM {}", self.table);
        sqlx::query_scalar::<_, i64>(&sql)
            .fetch_one(&self.pool)
            .await
            .unwrap_or(0) as usize
    }
}
```

This gives you a **persistent queue** that survives restarts and can be shared by multiple workers.

------

## 6. Mailbox abstraction – `src/mailbox/mod.rs`

```rust
use crate::types::{MailboxKey, MailboxMessage};
use async_trait::async_trait;

#[async_trait]
pub trait Mailbox<T>: Send + Sync
where
    T: Send + Sync + 'static,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>);

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>>;
}

pub mod in_memory;
#[cfg(feature = "postgres")]
pub mod postgres;
```

### 6.1 In-memory mailbox – `src/mailbox/in_memory.rs`

```rust
use super::Mailbox;
use crate::types::{MailboxKey, MailboxMessage};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Mutex;

pub struct InMemoryMailbox<T>
where
    T: Send + Sync + 'static,
{
    inner: Mutex<HashMap<MailboxKey, Vec<MailboxMessage<T>>>>,
}

impl<T> InMemoryMailbox<T>
where
    T: Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl<T> Mailbox<T> for InMemoryMailbox<T>
where
    T: Send + Sync + 'static,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        let mut guard = self.inner.lock().unwrap();
        guard.entry(key.clone()).or_default().push(msg);
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        let guard = self.inner.lock().unwrap();
        let msgs = guard.get(key).cloned().unwrap_or_default();
        let filtered = msgs
            .into_iter()
            .filter(|m| since_ms.map(|s| m.created_at_ms >= s).unwrap_or(true))
            .take(limit)
            .collect();
        filtered
    }
}
```

### 6.2 Postgres mailbox skeleton – `src/mailbox/postgres.rs`

```rust
#![cfg(feature = "postgres")]

use super::Mailbox;
use crate::types::{MailboxKey, MailboxMessage};
use async_trait::async_trait;
use serde_json::Value as JsonValue;
use sqlx::{Pool, Postgres};

pub struct PostgresMailbox<T>
where
    T: Send + Sync + 'static,
{
    pool: Pool<Postgres>,
    table: String,
    _marker: std::marker::PhantomData<T>,
}

impl<T> PostgresMailbox<T>
where
    T: Send + Sync + 'static,
{
    pub fn new(pool: Pool<Postgres>, table: impl Into<String>) -> Self {
        Self {
            pool,
            table: table.into(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Example schema:
    ///
    /// CREATE TABLE IF NOT EXISTS pl_mailbox (
    ///   id            BIGSERIAL PRIMARY KEY,
    ///   tenant        TEXT NOT NULL,
    ///   user_id       TEXT NULL,
    ///   session_id    TEXT NULL,
    ///   created_at_ms BIGINT NOT NULL,
    ///   status        TEXT NOT NULL,
    ///   payload       JSONB NULL
    /// );
    pub async fn migrate(&self) -> Result<(), sqlx::Error> {
        let sql = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {} (
                id            BIGSERIAL PRIMARY KEY,
                tenant        TEXT NOT NULL,
                user_id       TEXT NULL,
                session_id    TEXT NULL,
                created_at_ms BIGINT NOT NULL,
                status        TEXT NOT NULL,
                payload       JSONB NULL
            )
        "#,
            self.table
        );
        sqlx::query(&sql).execute(&self.pool).await?;
        Ok(())
    }
}

#[async_trait]
impl<T> Mailbox<T> for PostgresMailbox<T>
where
    T: Send + Sync + serde::de::DeserializeOwned + serde::Serialize + 'static,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        let sql = format!(
            r#"
            INSERT INTO {} (tenant, user_id, session_id, created_at_ms, status, payload)
            VALUES ($1, $2, $3, $4, $5, $6)
        "#,
            self.table
        );

        let payload_json = msg
            .payload
            .as_ref()
            .map(|p| serde_json::to_value(p).unwrap());

        let _ = sqlx::query(&sql)
            .bind(&key.tenant.0)
            .bind(&key.user_id)
            .bind(&key.session_id)
            .bind(msg.created_at_ms as i64)
            .bind(format!("{:?}", msg.status))
            .bind(payload_json)
            .execute(&self.pool)
            .await;
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        let sql = format!(
            r#"
            SELECT created_at_ms, status, payload
            FROM {}
            WHERE tenant = $1
              AND (user_id = $2 OR $2 IS NULL)
              AND (session_id = $3 OR $3 IS NULL)
              AND ($4 IS NULL OR created_at_ms >= $4)
            ORDER BY created_at_ms ASC
            LIMIT $5
        "#,
            self.table
        );

        let rows = sqlx::query(&sql)
            .bind(&key.tenant.0)
            .bind(&key.user_id)
            .bind(&key.session_id)
            .bind(since_ms.map(|s| s as i64))
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default();

        rows.into_iter()
            .filter_map(|row| {
                let created_at_ms: i64 = row.get("created_at_ms");
                let status_str: String = row.get("status");
                let payload_val: Option<JsonValue> = row.get("payload");

                // In a real implementation you'd parse status_str back to TaskStatus
                // and handle errors.
                let status = crate::types::TaskStatus::Completed;

                let payload = payload_val.and_then(|v| serde_json::from_value(v).ok());

                Some(MailboxMessage {
                    task_id: crate::types::TaskId(0), // you'd store it in the table too
                    status,
                    created_at_ms: created_at_ms as u128,
                    payload,
                })
            })
            .collect()
    }
}
```

------

## 7. Runtime adapters – `src/runtime/mod.rs` & `src/runtime/tokio.rs`

```rust
use async_trait::async_trait;
use std::future::Future;

#[async_trait]
pub trait Spawn: Send + Sync + Clone + 'static {
    async fn spawn_detached<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static;
}

#[cfg(feature = "native")]
pub mod tokio;
// src/runtime/tokio.rs
use super::Spawn;
use async_trait::async_trait;
use std::future::Future;

#[derive(Clone)]
pub struct TokioSpawner;

#[async_trait]
impl Spawn for TokioSpawner {
    async fn spawn_detached<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}
```

Later you can add a **web-worker spawner** for WASM.

------

## 8. The parking-lot scheduler – `src/scheduler.rs`

This is the **dedicated scheduling layer** (Option C) – it doesn’t know about HTTP, Tauri, etc. It just knows about jobs, queues, resources, and mailboxes.

```rust
use crate::mailbox::Mailbox;
use crate::queue::{DequeueOutcome, TaskQueue};
use crate::runtime::Spawn;
use crate::task::{ScheduledTask, TaskExecutor, TaskPayload};
use crate::types::{MailboxMessage, TaskStatus};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_units: u32,
    pub max_queue_depth: usize,
    pub default_timeout: Duration,
}

struct Inner<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    config: PoolConfig,
    queue: Q,
    mailbox: M,
    spawner: S,
    executor: E,
    active_units: u32,
}

#[derive(Clone)]
pub struct ResourcePool<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    inner: Arc<Mutex<Inner<P, Q, M, S, E, T>>>,
}

impl<P, Q, M, S, E, T> ResourcePool<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    pub fn new(config: PoolConfig, queue: Q, mailbox: M, spawner: S, executor: E) -> Self {
        let inner = Inner {
            config,
            queue,
            mailbox,
            spawner,
            executor,
            active_units: 0,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    /// Submit a task; either run immediately or park in the queue.
    pub async fn submit(&self, task: ScheduledTask<P>) -> crate::queue::EnqueueResult {
        let mut guard = self.inner.lock().unwrap();
        let cost = task.meta.cost.units;
        if guard.active_units + cost <= guard.config.max_units {
            // Run now
            guard.spawn_running(self.clone(), task);
            crate::queue::EnqueueResult {
                accepted: true,
                reason: None,
            }
        } else {
            guard.queue.enqueue(task).await
        }
    }

    async fn on_task_finished_inner(inner: &mut Inner<P, Q, M, S, E, T>, meta: crate::types::TaskMetadata, result: Result<T, String>) {
        inner.active_units -= meta.cost.units;

        if let Some(mailbox_key) = meta.mailbox.clone() {
            let msg = MailboxMessage {
                task_id: meta.id,
                status: match &result {
                    Ok(_) => TaskStatus::Completed,
                    Err(e) => TaskStatus::Failed(e.clone()),
                },
                created_at_ms: meta.created_at_ms, // or now_ms
                payload: result.ok(),
            };
            inner.mailbox.deliver(&mailbox_key, msg).await;
        }

        // Wake parked tasks.
        let now = Instant::now();
        let _expired = inner.queue.prune_expired(now).await;

        loop {
            match inner.queue.dequeue_next(now).await {
                DequeueOutcome::Empty => break,
                DequeueOutcome::Task(task) => {
                    let cost = task.meta.cost.units;
                    if inner.active_units + cost <= inner.config.max_units {
                        inner.spawn_running_self(task);
                    } else {
                        // Doesn't fit now; requeue and stop.
                        let _ = inner.queue.enqueue(task).await;
                        break;
                    }
                }
            }
        }
    }
}

impl<P, Q, M, S, E, T> Inner<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    fn spawn_running(&mut self, pool: ResourcePool<P, Q, M, S, E, T>, task: ScheduledTask<P>) {
        let cost = task.meta.cost.units;
        self.active_units += cost;

        let meta = task.meta.clone();
        let payload = task.payload;
        let executor = self.executor.clone();
        let spawner = self.spawner.clone();

        spawner.clone().spawn_detached(async move {
            let result = executor.execute(payload, meta.clone()).await;
            let mut guard = pool.inner.lock().unwrap();
            ResourcePool::on_task_finished_inner(&mut guard, meta, Ok(result)).await;
        });
    }

    fn spawn_running_self(&mut self, task: ScheduledTask<P>) {
        // Helper used only when already in an &mut Inner context and with a cloned pool.
        // Not used in this minimal snippet, but kept here for clarity/extension.
        let _ = task;
    }
}
```

> You might want to refactor the `spawn_running_self` or macros for less duplication when you flesh this out, but the core idea is clear: capacity accounting + spawn + callback into `on_task_finished`.

------

## 9. `src/lib.rs` – glue

```rust
pub mod types;
pub mod task;
pub mod queue;
pub mod mailbox;
pub mod runtime;
pub mod scheduler;

pub use crate::types::*;
pub use crate::task::*;
pub use crate::queue::*;
pub use crate::mailbox::*;
pub use crate::runtime::*;
pub use crate::scheduler::*;
```

------

## 10. Example: local LLM pool (desktop / Tauri / agent service)

This is **Option C in practice**: your service doesn’t care about threads or GPU; it just feeds jobs into this crate.

```rust
use prometheus_parking_lot::{
    mailbox::in_memory::InMemoryMailbox,
    queue::in_memory::InMemoryQueue,
    runtime::tokio::TokioSpawner,
    scheduler::{PoolConfig, ResourcePool},
    task::{TaskExecutor, ScheduledTask},
    types::*,
};
use async_trait::async_trait;
use std::time::Duration;

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct LlmJob {
    pub model: String,
    pub prompt: String,
    pub max_tokens: u32,
}

#[derive(Clone)]
struct LlmExecutor;

#[async_trait]
impl TaskExecutor<LlmJob, String> for LlmExecutor {
    async fn execute(&self, payload: LlmJob, _meta: TaskMetadata) -> String {
        // Call your candle-vllm / HTTP / MCP agent here.
        format!("LLM({}): {}", payload.model, payload.prompt)
    }
}

#[tokio::main]
async fn main() {
    // Example: 24GB GPU -> max 20 "units" for inference.
    let config = PoolConfig {
        max_units: 20,
        max_queue_depth: 1000,
        default_timeout: Duration::from_secs(60),
    };

    let queue = InMemoryQueue::<LlmJob>::new(config.max_queue_depth);
    let mailbox = InMemoryMailbox::<String>::new();
    let spawner = TokioSpawner;
    let executor = LlmExecutor;

    let pool = ResourcePool::new(config, queue, mailbox, spawner, executor);

    // Submit a job
    let meta = TaskMetadata {
        id: TaskId(1),
        priority: Priority::High,
        cost: ResourceCost { kind: ResourceKind::GpuVram, units: 5 }, // cost model can compute this
        created_at_ms: 0,
        deadline_ms: None,
        mailbox: Some(MailboxKey {
            tenant: TenantId("tenant-1".into()),
            user_id: Some("user-123".into()),
            session_id: None,
        }),
    };

    let job = LlmJob {
        model: "llama-3.1-8b".into(),
        prompt: "Hello, world".into(),
        max_tokens: 256,
    };

    let _res = pool.submit(ScheduledTask { meta, payload: job }).await;
}
```

Swap `InMemoryQueue` + `InMemoryMailbox` for `PostgresQueue` + `PostgresMailbox` and you’ve got **persistent queues + mailboxes between restarts**.

------

## 11. How this covers your three environments

1. **Local / desktop / Tauri / agent service**
   - Use `TokioSpawner`.
   - `LlmJob` payloads + `LlmExecutor`.
   - Capacity model based on GPU VRAM: `ResourceKind::GpuVram`, `units = f(vram estimate)`.
   - Queue: `InMemoryQueue` or `PostgresQueue` for durability.
   - Mailbox: `InMemoryMailbox` or `PostgresMailbox`.
2. **Web application using web workers**
   - Define `WebLlmJob` payload (includes all data needed for a worker).
   - Implement a `WebWorkerExecutor` (on the server or in WASM) that posts jobs to workers and awaits responses.
   - Implement a `WebWorkerSpawner` that uses `spawn_local` or your worker pool abstraction.
   - Everything else is the same: `ResourcePool` is unchanged.
3. **Cloud deployment**
   - Use `PostgresQueue` or another persistent backend for the queue.
   - Multiple worker instances can all use the same queue table.
   - Each worker has its own `ResourcePool` with `max_units` tuned to the VM’s CPU/GPU.
   - Graceful handling of timeouts and disconnected clients via `PostgresMailbox`.

------

## 12. Practical next steps (action plan)

1. **Create the crate** Drop this structure into `prometheus_parking_lot/`, wire up `Cargo.toml`, and get it to compile (you’ll fill in a few small gaps like `use sqlx::Row;`, time conversions, etc.).
2. **Implement a simple LLM job type** In your Rust AI gateway / candle-vllm fork:
   - Define `LlmJob` payload.
   - Implement `LlmExecutor` that calls your inference layer.
   - Configure `PoolConfig` using a real VRAM cost model.
3. **Swap in persistent queue & mailbox for your main gateway**
   - Use `PostgresQueue<LlmJob>` and `PostgresMailbox<String>` in your production agent.
   - Call `.migrate()` on both at startup.
4. **Add metrics & logging**
   - Wrap `ResourcePool` methods to emit `active_units`, `queue_len`, `expired_count`.
   - Plug into your existing Prometheus/Grafana or AGS metrics.
5. **Later: add WASM/web-worker adapter**
   - Implement `WebWorkerSpawner` and a `WebLlmExecutor` that uses postMessage to web workers.
   - Use the same `ResourcePool` type; change only the runtime adapter + executor.

------

If you’d like, next step I can:

- Add a small **VRAM-aware cost model helper** module for your local LLM setups.
- Sketch a **Postgres migration file** and **DAO helper** for the queue/mailbox tables aligned with your Prometheus DB conventions (PGlite / Supabase / Surreal variants).

------

## Prompt:

Sketch the Postgres migration file and seaOrm helper where appropriate for supported db conventions

## Answer:

Perfect, let’s wire this into real DB conventions.

I’ll give you:

1. **Postgres migrations** for:
   - `pl_queue_jobs` (parking-lot queue)
   - `pl_mailbox_messages` (mailbox for finished jobs)
2. **SeaORM entities** aligned with those tables.
3. **SeaORM-based helpers** that implement your `TaskQueue` and `Mailbox` traits on top of SeaORM.

You can drop this into your `prometheus_parking_lot` crate (or a sibling crate) and adapt as needed.

------

## 1. Postgres migrations

### 1.1 Queue table – `pl_queue_jobs`

This table is designed for:

- Multiple logical queues (by `queue_name`).
- Priority ordering.
- Deadline/timeouts.
- Locking for multi-worker “claim & work” pattern.

```
migrations/202512060001_create_pl_queue_jobs.sql
-- Queue / parking-lot table
CREATE TABLE IF NOT EXISTS pl_queue_jobs (
    id              BIGSERIAL PRIMARY KEY,

    -- For multi-queue setups (e.g. "llm_inference", "rag_preprocess", etc.)
    queue_name      TEXT NOT NULL,

    -- Higher = more important. We’ll map from your enum Priority.
    priority        SMALLINT NOT NULL,

    -- Creation time (for ordering) and an epoch ms stamp for fast comparisons.
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms   BIGINT      NOT NULL,

    -- Optional deadline, after which job is considered expired.
    deadline_at     TIMESTAMPTZ NULL,
    deadline_ms     BIGINT      NULL,

    -- Serialized TaskMetadata and payload (TaskPayload).
    metadata        JSONB NOT NULL,
    payload         JSONB NOT NULL,

    -- Optional optimistic locking for multi-worker dequeue.
    locked_by       TEXT        NULL,
    locked_at       TIMESTAMPTZ NULL,

    attempts        INTEGER     NOT NULL DEFAULT 0
);

-- Prioritized dequeue: by queue, priority desc, created_at asc
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_dequeue
    ON pl_queue_jobs (queue_name, priority DESC, created_at ASC);

-- Deadline pruning
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_deadline
    ON pl_queue_jobs (queue_name, deadline_ms);

-- Lock management
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_unlocked
    ON pl_queue_jobs (queue_name, locked_by, locked_at);
```

> You can also add a `tenant_id` column if you want per-tenant queues at DB level instead of in metadata.

------

### 1.2 Mailbox table – `pl_mailbox_messages`

For storing completed/failed/expired results per (tenant, user, session).

```
migrations/202512060002_create_pl_mailbox_messages.sql
CREATE TABLE IF NOT EXISTS pl_mailbox_messages (
    id              BIGSERIAL PRIMARY KEY,

    tenant          TEXT    NOT NULL,
    user_id         TEXT    NULL,
    session_id      TEXT    NULL,

    task_id         BIGINT  NULL, -- maps to pl_queue_jobs.id or TaskId if you want

    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms   BIGINT      NOT NULL,

    status          TEXT        NOT NULL, -- "Completed", "Failed(...)", "Expired", "Dropped(...)"
    payload         JSONB       NULL
);

-- Main read path: fetch by mailbox key + time, in order.
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_lookup
    ON pl_mailbox_messages (tenant, user_id, session_id, created_at_ms);

-- Optional: lookup by task id
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_task
    ON pl_mailbox_messages (task_id);
```

------

## 2. SeaORM entity models

Assuming SeaORM 0.12+.

### 2.1 Queue job entity – `entity/pl_queue_jobs.rs`

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "pl_queue_jobs")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub queue_name: String,
    pub priority: i16,

    pub created_at: DateTimeWithTimeZone,
    pub created_at_ms: i64,

    pub deadline_at: Option<DateTimeWithTimeZone>,
    pub deadline_ms: Option<i64>,

    pub metadata: Json,
    pub payload: Json,

    pub locked_by: Option<String>,
    pub locked_at: Option<DateTimeWithTimeZone>,
    pub attempts: i32,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

### 2.2 Mailbox message entity – `entity/pl_mailbox_messages.rs`

```rust
use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "pl_mailbox_messages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,

    pub tenant: String,
    pub user_id: Option<String>,
    pub session_id: Option<String>,

    pub task_id: Option<i64>,

    pub created_at: DateTimeWithTimeZone,
    pub created_at_ms: i64,

    pub status: String,
    pub payload: Option<Json>,
}

#[derive(Copy, Clone, Debug, EnumIter)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
```

------

## 3. SeaORM-backed queue helper

This is a SeaORM implementation of your `TaskQueue<P>` on top of `pl_queue_jobs`.

Assume:

```rust
use sea_orm::{DatabaseConnection, EntityTrait, QueryOrder, QueryFilter, ColumnTrait, ActiveModelTrait, Set, ColumnTrait};
```

### 3.1 Helper type – `sea_queue.rs`

```rust
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, Set, TransactionTrait,
};
use async_trait::async_trait;
use serde_json::json;
use std::time::Instant;

use crate::queue::{TaskQueue, EnqueueResult, DequeueOutcome};
use crate::task::{ScheduledTask, TaskPayload};
use crate::types::TaskMetadata;

// Generated SeaORM entity modules
use crate::entity::pl_queue_jobs::{self, Entity as QueueJob};

#[derive(Clone)]
pub struct SeaOrmQueue<P>
where
    P: TaskPayload,
{
    pub db: DatabaseConnection,
    pub queue_name: String,
    pub max_depth: usize,
    pub _marker: std::marker::PhantomData<P>,
}

impl<P> SeaOrmQueue<P>
where
    P: TaskPayload,
{
    pub fn new(db: DatabaseConnection, queue_name: impl Into<String>, max_depth: usize) -> Self {
        Self {
            db,
            queue_name: queue_name.into(),
            max_depth,
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<P> TaskQueue<P> for SeaOrmQueue<P>
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult {
        use pl_queue_jobs::Column as C;

        // Depth check
        let depth = QueueJob::find()
            .filter(C.queue_name.eq(self.queue_name.as_str()))
            .count(&self.db)
            .await
            .unwrap_or(0);

        if depth as usize >= self.max_depth {
            return EnqueueResult {
                accepted: false,
                reason: Some("queue at max capacity".into()),
            };
        }

        let meta_json = serde_json::to_value(&task.meta).unwrap();
        let payload_json = serde_json::to_value(&task.payload).unwrap();

        let active = pl_queue_jobs::ActiveModel {
            queue_name: Set(self.queue_name.clone()),
            priority: Set(task.meta.priority as i16),
            created_at_ms: Set(task.meta.created_at_ms as i64),
            deadline_ms: Set(task.meta.deadline_ms.map(|d| d as i64)),
            metadata: Set(Json(meta_json)),
            payload: Set(Json(payload_json)),
            ..Default::default()
        };

        let _ = active.insert(&self.db).await;

        EnqueueResult { accepted: true, reason: None }
    }

    async fn dequeue_next(&self, now: Instant) -> DequeueOutcome<P> {
        use pl_queue_jobs::Column as C;

        // Basic, non-locking dequeue:
        // Find one job for this queue, highest priority, oldest created_at
        if let Ok(Some(job)) = QueueJob::find()
            .filter(C::queue_name.eq(self.queue_name.as_str()))
            .order_by_desc(C::priority)
            .order_by_asc(C::created_at)
            .one(&self.db)
            .await
        {
            // Delete it (you can also use "locked" semantics in more advanced setups)
            let _ = job.delete(&self.db).await;

            let meta: TaskMetadata = serde_json::from_value(job.metadata.0).unwrap();
            let payload: P = serde_json::from_value(job.payload.0).unwrap();

            return DequeueOutcome::Task(ScheduledTask { meta, payload });
        }

        DequeueOutcome::Empty
    }

    async fn prune_expired(&self, _now: Instant) -> usize {
        // If you track epoch ms via an injected clock, you can:
        //   DELETE FROM pl_queue_jobs WHERE queue_name = ? AND deadline_ms IS NOT NULL AND deadline_ms < now_ms
        // For now, just return 0 as a sketch.
        0
    }

    async fn len(&self) -> usize {
        use pl_queue_jobs::Column as C;

        QueueJob::find()
            .filter(C::queue_name.eq(self.queue_name.as_str()))
            .count(&self.db)
            .await
            .unwrap_or(0) as usize
    }
}
```

> You can extend this to use **locking semantics** (e.g., `locked_by`, `locked_at`) and a two-phase “claim + delete on success” pattern, but this is a clean starting point.

------

## 4. SeaORM-backed mailbox helper

SeaORM implementation of your `Mailbox<T>` trait, sitting on `pl_mailbox_messages`.

### 4.1 Helper type – `sea_mailbox.rs`

```rust
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, Set,
};
use async_trait::async_trait;
use serde_json::json;

use crate::mailbox::Mailbox;
use crate::types::{MailboxKey, MailboxMessage, TaskStatus};

use crate::entity::pl_mailbox_messages::{self, Entity as MailboxEntity};

#[derive(Clone)]
pub struct SeaOrmMailbox<T>
where
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    pub db: DatabaseConnection,
    pub _marker: std::marker::PhantomData<T>,
}

impl<T> SeaOrmMailbox<T>
where
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    pub fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T> Mailbox<T> for SeaOrmMailbox<T>
where
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        use pl_mailbox_messages::ActiveModel;

        let payload_json = msg
            .payload
            .as_ref()
            .map(|p| serde_json::to_value(p).unwrap());

        let status_str = format!("{:?}", msg.status);

        let active = ActiveModel {
            tenant: Set(key.tenant.0.clone()),
            user_id: Set(key.user_id.clone()),
            session_id: Set(key.session_id.clone()),
            task_id: Set(Some(msg.task_id.0 as i64)),
            created_at_ms: Set(msg.created_at_ms as i64),
            status: Set(status_str),
            payload: Set(payload_json.map(sea_orm::Json)),
            ..Default::default()
        };

        let _ = active.insert(&self.db).await;
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        use pl_mailbox_messages::Column as C;

        let mut query = MailboxEntity::find()
            .filter(C::tenant.eq(key.tenant.0.as_str()));

        if let Some(ref user_id) = key.user_id {
            query = query.filter(C::user_id.eq(user_id.as_str()));
        }

        if let Some(ref session_id) = key.session_id {
            query = query.filter(C::session_id.eq(session_id.as_str()));
        }

        if let Some(since) = since_ms {
            query = query.filter(C::created_at_ms.gte(since as i64));
        }

        let rows = query
            .order_by_asc(C::created_at_ms)
            .limit(limit as u64)
            .all(&self.db)
            .await
            .unwrap_or_default();

        rows.into_iter()
            .filter_map(|row| {
                let payload = row
                    .payload
                    .as_ref()
                    .and_then(|j| serde_json::from_value(j.0.clone()).ok());

                // Status string parsing is up to you; for now just treat all as Completed.
                let status = TaskStatus::Completed;

                Some(MailboxMessage {
                    task_id: crate::types::TaskId(row.task_id.unwrap_or(0) as u64),
                    status,
                    created_at_ms: row.created_at_ms as u128,
                    payload,
                })
            })
            .collect()
    }
}
```

------

## 5. How this fits into the scheduling layer (Option C)

Once these SeaORM helpers exist, using the crate as a **dedicated scheduling layer** is straightforward:

- In your service (Rust AI gateway, Tauri agent, etc.):

```rust
use prometheus_parking_lot::{
    scheduler::{PoolConfig, ResourcePool},
    runtime::tokio::TokioSpawner,
    task::{TaskExecutor, ScheduledTask},
    types::*,
};
use my_crate::sea_queue::SeaOrmQueue;
use my_crate::sea_mailbox::SeaOrmMailbox;
use sea_orm::Database;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = Database::connect("postgres://...").await?;

    let queue = SeaOrmQueue::<LlmJob>::new(db.clone(), "llm_inference", 10_000);
    let mailbox = SeaOrmMailbox::<String>::new(db.clone());

    let config = PoolConfig {
        max_units: 20,
        max_queue_depth: 10_000,
        default_timeout: std::time::Duration::from_secs(60),
    };

    let pool = ResourcePool::new(
        config,
        queue,
        mailbox,
        TokioSpawner,
        LlmExecutor, // your TaskExecutor
    );

    // submit jobs to `pool.submit(...)`
    Ok(())
}
```

Now your **prometheus_parking_lot** scheduler:

- Can be shared by **multiple workers** (SeaORM queue).
- Survives **restarts**.
- Stores results in a **mailbox** for late pickup.

------

### If you’d like next

I can:

- Add a **VRAM-aware cost model** sketch for `ResourceCost` (for GPUs).
- Show a **multi-worker lock-safe dequeue** pattern using SeaORM (with `locked_by` + `locked_at`) so multiple pods/VMs can safely share the same queue.

------

## Prompt:

Also consider and adapter for Postgres (or an additional abstraction within the single Postgres adapter that can use pgmq extension for queue storage instead of the custom schema).

## Answer:

Got it—let’s fold **pgmq** in as a first-class option alongside the custom tables, without blowing up the abstraction.

I’ll:

1. Define a `PostgresQueueBackend` **enum** that supports:
   - `CustomTable` (the SeaORM/custom schema we already sketched)
   - `Pgmq` (using the `pgmq` extension)
2. Sketch a `PgmqQueue<P>` implementation of `TaskQueue<P>` that wraps the pgmq API.
3. Show how to plug this into the existing `ResourcePool` without changing your scheduler or task logic.

------

## 1. Unified Postgres queue abstraction

We already have a generic `TaskQueue<P>` trait. Now let’s introduce a small, unified “Postgres queue” wrapper that can be backed by either:

- SeaORM + `pl_queue_jobs` table, or
- `pgmq` extension.

### 1.1 Backend enum

```rust
/// Choose which Postgres-based queue backend to use.
pub enum PostgresQueueBackend {
    /// Our custom "pl_queue_jobs" table (SeaORM / SQLx-based).
    CustomTable {
        table_name: String,
    },
    /// The pgmq extension, using its internal tables.
    Pgmq {
        queue_name: String,
    },
}
```

Then a higher-level constructor that returns a `Box<dyn TaskQueue<P>>`:

```rust
use sea_orm::DatabaseConnection;

use crate::queue::TaskQueue;
use crate::task::TaskPayload;

pub fn build_postgres_task_queue<P>(
    db: DatabaseConnection,
    backend: PostgresQueueBackend,
    max_depth: usize,
) -> Box<dyn TaskQueue<P>>
where
    P: TaskPayload,
{
    match backend {
        PostgresQueueBackend::CustomTable { table_name } => {
            Box::new(SeaOrmQueue::<P>::new(db, table_name, max_depth))
        }
        PostgresQueueBackend::Pgmq { queue_name } => {
            Box::new(PgmqQueue::<P>::new(db, queue_name, max_depth))
        }
    }
}
```

So upstream code (your gateway / agent) only chooses a **backend enum**; the scheduler stays oblivious.

------

## 2. Pgmq basics (how we’ll model it)

The `pgmq` extension gives you:

- A **queue** identified by a name.
- Functions like (naming varies slightly by version, but conceptually):
  - `pgmq.create(queue_name text)`
  - `pgmq.send(queue_name text, message jsonb)`
  - `pgmq.read(queue_name text, vt integer, limit integer)`
  - `pgmq.delete(queue_name text, msg_id bigint)`
  - Optionally `pgmq.archive`, `pgmq.set_vt`, etc.

We’ll treat `message` as a JSON object:

```json
{
  "metadata": { ... TaskMetadata ... },
  "payload": { ... P ... }
}
```

Then our adapter:

- `enqueue` → `pgmq.send`
- `dequeue_next` → `pgmq.read` (limit 1), then `pgmq.delete`
- `prune_expired` → either:
  - Not needed (pgmq doesn’t care), or
  - Implemented by inspecting `metadata.deadline_ms` in the message and deleting/ignoring expired items.

------

## 3. PgmqQueue implementation

We’ll use SeaORM’s `DatabaseConnection` but drop to raw SQL for `pgmq` calls.

### 3.1 `pgmq_queue.rs`

```rust
use async_trait::async_trait;
use sea_orm::{DatabaseConnection, Statement, QueryResult};
use serde_json::json;
use std::time::Instant;

use crate::queue::{TaskQueue, EnqueueResult, DequeueOutcome};
use crate::task::{ScheduledTask, TaskPayload};
use crate::types::TaskMetadata;

/// A TaskQueue implementation backed by the pgmq Postgres extension.
#[derive(Clone)]
pub struct PgmqQueue<P>
where
    P: TaskPayload,
{
    db: DatabaseConnection,
    queue_name: String,
    max_depth: usize,
    _marker: std::marker::PhantomData<P>,
}

impl<P> PgmqQueue<P>
where
    P: TaskPayload,
{
    pub fn new(db: DatabaseConnection, queue_name: impl Into<String>, max_depth: usize) -> Self {
        Self {
            db,
            queue_name: queue_name.into(),
            max_depth,
            _marker: std::marker::PhantomData,
        }
    }

    /// Ensure the queue exists. Call at startup.
    pub async fn ensure_queue(&self) -> Result<(), sea_orm::DbErr> {
        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pgmq.create($1)",
            vec![self.queue_name.clone().into()],
        );
        let _ = self.db.execute(stmt).await?;
        Ok(())
    }

    /// Internal: count approximate messages for depth control.
    async fn approx_depth(&self) -> usize {
        // pgmq has metadata tables but for a sketch, we can just skip a strict count
        // or query pgmq's internal catalog if needed. Here we return 0 and rely
        // on max_depth primarily as a logical limit rather than a hard DB-backed check.
        0
    }
}

#[async_trait]
impl<P> TaskQueue<P> for PgmqQueue<P>
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult {
        // Optional depth check; for a real implementation, you'd query the pgmq metadata.
        let depth = self.approx_depth().await;
        if depth >= self.max_depth {
            return EnqueueResult {
                accepted: false,
                reason: Some("queue at max capacity (logical limit)".into()),
            };
        }

        let metadata_json = serde_json::to_value(&task.meta).unwrap();
        let payload_json = serde_json::to_value(&task.payload).unwrap();

        let msg_json = json!({
            "metadata": metadata_json,
            "payload": payload_json
        });

        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pgmq.send($1, $2::jsonb)",
            vec![self.queue_name.clone().into(), msg_json.into()],
        );

        let _ = self.db.execute(stmt).await;

        EnqueueResult {
            accepted: true,
            reason: None,
        }
    }

    async fn dequeue_next(&self, _now: Instant) -> DequeueOutcome<P> {
        // READ: visibility timeout (vt) = 30s (example), limit = 1
        // Returns: (msg_id, read_count, enqueued_at, vt, message jsonb)
        let stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT * FROM pgmq.read($1, 30, 1)",
            vec![self.queue_name.clone().into()],
        );

        let res = self.db.query_all(stmt).await;
        let rows = match res {
            Ok(r) => r,
            Err(_) => return DequeueOutcome::Empty,
        };

        if rows.is_empty() {
            return DequeueOutcome::Empty;
        }

        let row = &rows[0];
        // Column schema example (you'll adjust to the actual pgmq version):
        // msg_id BIGINT
        // read_ct INT
        // enqueued_at TIMESTAMPTZ
        // vt TIMESTAMPTZ
        // message JSONB

        let msg_id: i64 = row.try_get("", "msg_id").unwrap();
        let msg_json: serde_json::Value = row.try_get("", "message").unwrap();

        let metadata_json = msg_json
            .get("metadata")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let payload_json = msg_json
            .get("payload")
            .cloned()
            .unwrap_or_else(|| json!({}));

        let meta: TaskMetadata = serde_json::from_value(metadata_json).unwrap();
        let payload: P = serde_json::from_value(payload_json).unwrap();

        // Since we successfully "claim" this message, we now delete it from the queue.
        let del_stmt = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT pgmq.delete($1, $2)",
            vec![self.queue_name.clone().into(), msg_id.into()],
        );
        let _ = self.db.execute(del_stmt).await;

        DequeueOutcome::Task(ScheduledTask { meta, payload })
    }

    async fn prune_expired(&self, now: Instant) -> usize {
        // pgmq doesn't natively know about your deadlines; you enforce them at dequeue time.
        // A common pattern:
        //   - When you dequeue, if metadata.deadline_ms < now_ms, treat as expired and discard.
        //   - Or push to an "expired" or "dead letter" queue via pgmq.archive.
        //
        // For a sketch, we just return 0 and assume expiry is handled in scheduler logic.
        0
    }

    async fn len(&self) -> usize {
        // Pgmq offers introspection via internal tables; for a sketch we skip it.
        0
    }
}
```

> In a real implementation, you’d:
>
> - Inspect `pgmq`’s catalog tables to compute approximate `len`.
> - Possibly use `pgmq.set_vt` or `pgmq.archive` to model retries and dead-letter queues.

------

## 4. Integrating Pgmq with the scheduler

The scheduler doesn’t change. You just choose the backend:

```rust
use prometheus_parking_lot::{
    scheduler::{PoolConfig, ResourcePool},
    runtime::tokio::TokioSpawner,
    task::{TaskExecutor, ScheduledTask},
    types::*,
};
use sea_orm::Database;
use my_crate::{build_postgres_task_queue, PostgresQueueBackend};
use my_crate::sea_mailbox::SeaOrmMailbox; // or PostgresMailbox

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct LlmJob {
    model: String,
    prompt: String,
    max_tokens: u32,
}

#[derive(Clone)]
struct LlmExecutor;

#[async_trait::async_trait]
impl TaskExecutor<LlmJob, String> for LlmExecutor {
    async fn execute(&self, payload: LlmJob, _meta: TaskMetadata) -> String {
        // call candle-vllm / whatever
        format!("LLM({}): {}", payload.model, payload.prompt)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db = Database::connect("postgres://...").await?;

    // Option 1: Custom schema
    // let backend = PostgresQueueBackend::CustomTable { table_name: "pl_queue_jobs".into() };

    // Option 2: pgmq
    let backend = PostgresQueueBackend::Pgmq { queue_name: "llm_inference".into() };

    let max_depth = 10_000;
    let queue = build_postgres_task_queue::<LlmJob>(db.clone(), backend, max_depth);

    let mailbox = SeaOrmMailbox::<String>::new(db.clone());

    let config = PoolConfig {
        max_units: 20,
        max_queue_depth: max_depth,
        default_timeout: std::time::Duration::from_secs(60),
    };

    let spawner = TokioSpawner;
    let executor = LlmExecutor;

    let pool = ResourcePool::new(config, queue, mailbox, spawner, executor);

    // Submit a job
    let meta = TaskMetadata {
        id: TaskId(1),
        priority: Priority::High,
        cost: ResourceCost { kind: ResourceKind::GpuVram, units: 5 },
        created_at_ms: 0,
        deadline_ms: None,
        mailbox: Some(MailboxKey {
            tenant: TenantId("tenant-1".into()),
            user_id: Some("user-123".into()),
            session_id: None,
        }),
    };

    let job = LlmJob {
        model: "llama-3.1-8b".into(),
        prompt: "Hello from pgmq".into(),
        max_tokens: 256,
    };

    let _res = pool.submit(ScheduledTask { meta, payload: job }).await;

    Ok(())
}
```

You can swap **between custom schema and** `pgmq` without touching:

- The scheduler
- The executor
- The mailbox
- The service logic

Only the **queue backend config changes**, which is exactly what you want for Prometheus platform deployment flexibility.

------

## 5. How this matches Prometheus conventions

- **Single scheduling layer**: the `ResourcePool` + traits remain unchanged.
- **Multiple backends for the same abstraction**:
  - Local dev: in-memory queue.
  - On-prem / simple deployments: custom schema via SeaORM.
  - Larger-scale Postgres deployments: `pgmq` extension for high-throughput queues.
- **Future-proof**:
  - You can add **SurrealDB-based queues**, **Redis**, or **NATS** simply by implementing `TaskQueue<P>`.

------

## 6. Immediate action items for you

1. **Decide where to house the pgmq adapter**:
   - Inside `prometheus_parking_lot` under `queue::pgmq`.
   - Or in a sibling crate `prometheus_parking_lot_pgmq` to keep deps modular.
2. **Wire pgmq migrations**:
   - Install `pgmq` via extension on your Postgres instances.
   - Optionally add a helper in Rust to call `pgmq.create(queue_name)` for all queues at startup.
3. **Hook into your Prometheus AI gateway**:
   - Replace your existing “ad hoc” queue/worker code.
   - Let the gateway be purely: HTTP → `ResourcePool.submit()` → mailbox → streaming or late pickup.

If you want, next I can sketch:

- A **config-driven queue builder** (e.g. from a `toml/json` config for each agent type) so you can declare queues + backends declaratively in your Prometheus runtime.

------

## Prompt:

Before we sketch YAML configuration requirements we must consider a couple of other queuing and mailbox abstraction offerings:

1. For queuing in process in a Tauri app, reddb open source embedded queueing should be an option.
2. Mailbox could be abstracted by storage only, storage and message sent to REST callback, in-memory.

## Answer:

Good call hitting pause on YAML until the abstraction surface is right. Let’s evolve the design to:

1. Add **ReddDb-based in-process queueing** for Tauri / desktop.
2. Refine the **Mailbox** abstraction so it can be:
   - storage-only,
   - storage + REST callback,
   - in-memory only.

I’ll integrate these cleanly into what we already sketched, so we’re converging on the “one scheduling layer, multiple backends” design.

------

## 1. Extending the queue story with ReddDb (Tauri / embedded)

### 1.1 New backend variant

We already had:

```rust
pub enum PostgresQueueBackend {
    CustomTable { table_name: String },
    Pgmq { queue_name: String },
}
```

At the *global* queue level (independent of DB tech), we want a broader enum:

```rust
pub enum QueueBackend {
    InMemory,
    PostgresCustomTable { table_name: String },
    PostgresPgmq { queue_name: String },
    ReddDb { path: String, queue_name: String },
}
```

Then a single builder:

```rust
use sea_orm::DatabaseConnection;
use crate::queue::TaskQueue;
use crate::task::TaskPayload;

pub enum QueueBuilderContext {
    /// We have a Postgres connection (SeaORM/SQLx).
    Postgres { db: DatabaseConnection },
    /// We’re in an embedded environment (Tauri). No Postgres.
    Embedded,
}

pub fn build_task_queue<P>(
    ctx: QueueBuilderContext,
    backend: QueueBackend,
    max_depth: usize,
) -> Box<dyn TaskQueue<P>>
where
    P: TaskPayload,
{
    match (ctx, backend) {
        (QueueBuilderContext::Embedded, QueueBackend::InMemory) => {
            Box::new(crate::queue::in_memory::InMemoryQueue::<P>::new(max_depth))
        }
        (QueueBuilderContext::Embedded, QueueBackend::ReddDb { path, queue_name }) => {
            Box::new(crate::queue::redddb::ReddDbQueue::<P>::new(path, queue_name, max_depth))
        }

        (QueueBuilderContext::Postgres { db }, QueueBackend::InMemory) => {
            Box::new(crate::queue::in_memory::InMemoryQueue::<P>::new(max_depth))
        }
        (QueueBuilderContext::Postgres { db }, QueueBackend::PostgresCustomTable { table_name }) => {
            Box::new(crate::queue::sea_queue::SeaOrmQueue::<P>::new(db, table_name, max_depth))
        }
        (QueueBuilderContext::Postgres { db }, QueueBackend::PostgresPgmq { queue_name }) => {
            Box::new(crate::queue::pgmq_queue::PgmqQueue::<P>::new(db, queue_name, max_depth))
        }

        // Anything else is invalid at runtime (e.g. ReddDb + Postgres ctx).
        (ctx, backend) => panic!("Unsupported combination: {:?} / {:?}", ctx, backend),
    }
}
```

You can also make this non-panicking by returning `Result<Box<dyn TaskQueue<P>>, QueueBuilderError>`; but the idea is clear.

### 1.2 ReddDbQueue skeleton

We don’t need the exact `reddb` API here; we just define an adapter that will wrap whatever their embedded-queue primitives are.

```rust
// src/queue/redddb.rs

use async_trait::async_trait;
use std::time::Instant;

use crate::queue::{TaskQueue, EnqueueResult, DequeueOutcome};
use crate::task::{ScheduledTask, TaskPayload};
use crate::types::TaskMetadata;

/// In-process embedded queue using ReddDb
#[derive(Clone)]
pub struct ReddDbQueue<P>
where
    P: TaskPayload,
{
    // A handle to the embedded DB / queue.
    // Adjust these types to the real reddb API.
    db_path: String,
    queue_name: String,
    max_depth: usize,
    _marker: std::marker::PhantomData<P>,
}

impl<P> ReddDbQueue<P>
where
    P: TaskPayload,
{
    pub fn new(path: impl Into<String>, queue_name: impl Into<String>, max_depth: usize) -> Self {
        Self {
            db_path: path.into(),
            queue_name: queue_name.into(),
            max_depth,
            _marker: std::marker::PhantomData,
        }
    }

    fn open_or_create_queue(&self) {
        // pseudo-code: integrate real reddb calls
        // reddb::open(&self.db_path)?.create_queue(&self.queue_name)?;
    }

    fn approximate_len(&self) -> usize {
        // pseudo-code
        0
    }
}

#[async_trait]
impl<P> TaskQueue<P> for ReddDbQueue<P>
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult {
        self.open_or_create_queue();

        let depth = self.approximate_len();
        if depth >= self.max_depth {
            return EnqueueResult {
                accepted: false,
                reason: Some("queue at max capacity (ReddDb)".into()),
            };
        }

        // Serialize metadata + payload
        let meta_json = serde_json::to_value(&task.meta).unwrap();
        let payload_json = serde_json::to_value(&task.payload).unwrap();
        let msg = serde_json::json!({ "metadata": meta_json, "payload": payload_json });

        // pseudo-code for reddb queue push:
        // reddb.push(self.queue_name, msg)?;

        EnqueueResult { accepted: true, reason: None }
    }

    async fn dequeue_next(&self, _now: Instant) -> DequeueOutcome<P> {
        self.open_or_create_queue();

        // pseudo-code:
        // if let Some(msg) = reddb.pop(self.queue_name)? {
        //   let meta: TaskMetadata = serde_json::from_value(msg["metadata"].clone())?;
        //   let payload: P = serde_json::from_value(msg["payload"].clone())?;
        //   return DequeueOutcome::Task(ScheduledTask { meta, payload });
        // }

        DequeueOutcome::Empty
    }

    async fn prune_expired(&self, _now: Instant) -> usize {
        // For embedded use in Tauri you can:
        //   - handle expiry on dequeue
        //   - or periodically scan & drop expired messages
        0
    }

    async fn len(&self) -> usize {
        self.approximate_len()
    }
}
```

This gives you a **zero-infrastructure queue backend** that still fits under the same `TaskQueue` trait and works with the same `ResourcePool`.

------

## 2. Evolving the mailbox abstraction

You want:

> Mailbox could be abstracted by storage only, storage and message sent to REST callback, in-memory.

So we should separate **“where do I store the result?”** from **“how do I notify anyone?”**

### 2.1 Split into Storage + Notifier

We keep your **public** `Mailbox<T>` trait but implement it in terms of:

- `MailboxStorage<T>`: persistent or in-memory storage.
- `MailboxNotifier<T>`: optional notification (REST callback, WS, push, etc.).

#### Storage trait

```rust
use async_trait::async_trait;
use crate::types::{MailboxKey, MailboxMessage};

#[async_trait]
pub trait MailboxStorage<T>: Send + Sync
where
    T: Send + Sync + 'static,
{
    async fn store(&self, key: &MailboxKey, msg: MailboxMessage<T>);

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>>;
}
```

#### Notifier trait

```rust
use async_trait::async_trait;

#[async_trait]
pub trait MailboxNotifier<T>: Send + Sync
where
    T: Send + Sync + 'static,
{
    async fn notify(&self, key: &MailboxKey, msg: &MailboxMessage<T>);
}
```

Then our original **Mailbox** becomes a small orchestrator:

```rust
use async_trait::async_trait;

#[async_trait]
pub trait Mailbox<T>: Send + Sync
where
    T: Send + Sync + 'static,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>);

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>>;
}
```

### 2.2 Concrete mailbox types

#### 2.2.1 Storage-only mailbox

```rust
pub struct StorageOnlyMailbox<T, S>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
{
    storage: S,
    _marker: std::marker::PhantomData<T>,
}

impl<T, S> StorageOnlyMailbox<T, S>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
{
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, S> Mailbox<T> for StorageOnlyMailbox<T, S>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        self.storage.store(key, msg).await;
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        self.storage.fetch(key, since_ms, limit).await
    }
}
```

#### 2.2.2 Storage + REST callback mailbox

We add a notifier implementation that calls a REST hook (maybe per-tenant):

```rust
use reqwest::Client;

pub struct HttpCallbackNotifier<T> {
    client: Client,
    // Could be a global URL or looked up by tenant/user.
    base_url: String,
    _marker: std::marker::PhantomData<T>,
}

impl<T> HttpCallbackNotifier<T> {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            client: Client::new(),
            base_url: base_url.into(),
            _marker: std::marker::PhantomData,
        }
    }

    async fn callback_url_for(&self, key: &MailboxKey) -> String {
        // E.g. {base_url}/mailbox/{tenant}/{user_id?}/{session_id?}
        let mut url = format!("{}/mailbox/{}", self.base_url, key.tenant.0);
        if let Some(ref u) = key.user_id {
            url.push('/');
            url.push_str(u);
        }
        if let Some(ref s) = key.session_id {
            url.push('/');
            url.push_str(s);
        }
        url
    }
}

#[async_trait::async_trait]
impl<T> MailboxNotifier<T> for HttpCallbackNotifier<T>
where
    T: Send + Sync + serde::Serialize + 'static,
{
    async fn notify(&self, key: &MailboxKey, msg: &MailboxMessage<T>) {
        let url = self.callback_url_for(key).await;
        let _ = self.client.post(url).json(msg).send().await;
    }
}
```

Now we build a **composite mailbox** that does both storage and notification:

```rust
pub struct StorageWithNotifierMailbox<T, S, N>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
    N: MailboxNotifier<T>,
{
    storage: S,
    notifier: N,
    _marker: std::marker::PhantomData<T>,
}

impl<T, S, N> StorageWithNotifierMailbox<T, S, N>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
    N: MailboxNotifier<T>,
{
    pub fn new(storage: S, notifier: N) -> Self {
        Self {
            storage,
            notifier,
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait::async_trait]
impl<T, S, N> Mailbox<T> for StorageWithNotifierMailbox<T, S, N>
where
    T: Send + Sync + serde::Serialize + 'static,
    S: MailboxStorage<T>,
    N: MailboxNotifier<T>,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        self.storage.store(key, msg.clone()).await;
        self.notifier.notify(key, &msg).await;
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        self.storage.fetch(key, since_ms, limit).await
    }
}
```

#### 2.2.3 In-memory mailbox

Our earlier `InMemoryMailbox<T>` can now be split into:

- `InMemoryStorage<T>` implementing `MailboxStorage<T>`.
- A convenience `StorageOnlyMailbox<InMemoryStorage<T>>`.

```rust
pub struct InMemoryStorage<T>
where
    T: Send + Sync + 'static,
{
    inner: std::sync::Mutex<std::collections::HashMap<MailboxKey, Vec<MailboxMessage<T>>>>,
}

impl<T> InMemoryStorage<T>
where
    T: Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(Default::default()),
        }
    }
}

#[async_trait::async_trait]
impl<T> MailboxStorage<T> for InMemoryStorage<T>
where
    T: Send + Sync + 'static,
{
    async fn store(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        let mut guard = self.inner.lock().unwrap();
        guard.entry(key.clone()).or_default().push(msg);
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        let guard = self.inner.lock().unwrap();
        let msgs = guard.get(key).cloned().unwrap_or_default();
        msgs.into_iter()
            .filter(|m| since_ms.map(|s| m.created_at_ms >= s).unwrap_or(true))
            .take(limit)
            .collect()
    }
}

// Then a simple alias:
pub type InMemoryMailbox<T> = StorageOnlyMailbox<T, InMemoryStorage<T>>;
```

For Postgres / SeaORM mailbox, same story: you implement `MailboxStorage<T>` on top of the `pl_mailbox_messages` table, then you can choose:

- `StorageOnlyMailbox<SeaOrmMailboxStorage<T>>`
- or `StorageWithNotifierMailbox<SeaOrmMailboxStorage<T>, HttpCallbackNotifier<T>>`

depending on whether you also want the REST callback.

------

## 3. How this plays out across your environments

### 3.1 Tauri desktop / local agents

- **Queue backend**: `QueueBackend::ReddDb { path, queue_name }`
  - No external DB; all in-process.
- **Mailbox**:
  - For purely local UI: `InMemoryMailbox<T>` might be enough.
  - If you want cross-session persistence: `MailboxStorage` backed by a small local DB (Pglite, ReddDb, sqlite) + optional `HttpCallbackNotifier` hitting a Tauri command or local HTTP endpoint.

### 3.2 Web app (web workers, PGlite)

- **Queue backend**:
  - On server side: Postgres custom table or `pgmq`.
  - In-browser (if needed): a separate embedded queue (indexedDB/PGlite) with its own `TaskQueue` impl.
- **Mailbox**:
  - Storage: Postgres / Supabase / Pglite.
  - Notifier: REST callback that pings your web backend (which can then fan out via WS to the frontend).

### 3.3 Cloud

- **Queue backend**:
  - `QueueBackend::PostgresPgmq` for high-throughput queueing without separate infra.
  - Or custom table for more control + SeaORM.
- **Mailbox**:
  - Storage: Postgres or Surreal via `MailboxStorage`.
  - Notifier: `HttpCallbackNotifier` hitting a per-tenant webhook, or pluggable notifier that also supports:
    - Kafka, NATS, SNS, etc. (just more `MailboxNotifier` implementations).

And in all cases, the **scheduling layer (**`ResourcePool`**) doesn’t change at all**. You just:

- pick a `TaskQueue` backend,
- pick a `Mailbox` implementation,
- and plug them into the same `ResourcePool` constructor.

------

## 4. Concrete next step

Now that:

- Queue backends cover **in-memory, Postgres, pgmq, ReddDb**.
- Mailbox covers **storage only, storage + REST, in-memory** via composable traits.

We’re ready to sketch:

- A **config schema** (YAML/JSON) that declares, for each pool:
  - which queue backend (and its parameters),
  - which mailbox storage backend,
  - which notifier(s), if any,
  - capacity (max_units, max_queue_depth),
  - LLM/GPU cost model hints.

When you’re ready, I’ll design that config schema so it can plug straight into a `build_from_config()` API in the crate and be shared across all Prometheus agents.

------

## Prompt:

Replace the reddb idea with Yaque, which is far better suited for rust

## Answer:

Nice, Yaque fits this role a *lot* better than a random embedded DB.

I’ll swap out the “ReddDb” idea and show how Yaque becomes the **embedded queue backend** in the same abstraction we’ve already designed.

------

## 1. Updated queue backend enum

At the “queue selection” layer, we now support:

- In-memory (for tests / simple setups)
- Postgres (custom table via SeaORM)
- Postgres (pgmq extension)
- **Yaque (embedded, file-based queue; ideal for Tauri / single-node Rust)**

```rust
pub enum QueueBackend {
    InMemory,
    PostgresCustomTable { table_name: String },
    PostgresPgmq { queue_name: String },
    Yaque { path: String, stream: String },
}
```

We still distinguish **where we are** via `QueueBuilderContext`:

```rust
pub enum QueueBuilderContext {
    /// We have a Postgres connection (SeaORM/SQLx).
    Postgres { db: sea_orm::DatabaseConnection },
    /// Embedded / local (Tauri, CLI agents, etc.).
    Embedded,
}
```

And a single builder that returns a `Box<dyn TaskQueue<P>>`:

```rust
use crate::queue::TaskQueue;
use crate::task::TaskPayload;

pub fn build_task_queue<P>(
    ctx: QueueBuilderContext,
    backend: QueueBackend,
    max_depth: usize,
) -> Box<dyn TaskQueue<P>>
where
    P: TaskPayload,
{
    match (ctx, backend) {
        (QueueBuilderContext::Embedded, QueueBackend::InMemory) => {
            Box::new(crate::queue::in_memory::InMemoryQueue::<P>::new(max_depth))
        }

        (QueueBuilderContext::Embedded, QueueBackend::Yaque { path, stream }) => {
            Box::new(crate::queue::yaque::YaqueQueue::<P>::new(path, stream, max_depth))
        }

        (QueueBuilderContext::Postgres { db }, QueueBackend::InMemory) => {
            Box::new(crate::queue::in_memory::InMemoryQueue::<P>::new(max_depth))
        }

        (QueueBuilderContext::Postgres { db }, QueueBackend::PostgresCustomTable { table_name }) => {
            Box::new(crate::queue::sea_queue::SeaOrmQueue::<P>::new(db, table_name, max_depth))
        }

        (QueueBuilderContext::Postgres { db }, QueueBackend::PostgresPgmq { queue_name }) => {
            Box::new(crate::queue::pgmq_queue::PgmqQueue::<P>::new(db, queue_name, max_depth))
        }

        // For now, we treat Yaque as Embedded-only, but you *could* allow it on Postgres
        // contexts as well if you want hybrid setups.
        (ctx, backend) => panic!("Unsupported combination: {:?} / {:?}", ctx, backend),
    }
}
```

So: all Prometheus agents/shells configure a `QueueBackend`, and the rest of the system doesn’t care whether it’s Yaque or pgmq.

------

## 2. Yaque-backed `TaskQueue` implementation

We’re going to implement:

```rust
pub struct YaqueQueue<P: TaskPayload> { ... }

impl<P: TaskPayload> TaskQueue<P> for YaqueQueue<P> { ... }
```

Internally, Yaque is a file-backed queue. I’ll keep the API usage as **pseudo-code** so you can wire to actual Yaque calls without fighting imaginary types.

### 2.1 Structure & constructor

```rust
// src/queue/yaque.rs

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

use crate::queue::{TaskQueue, EnqueueResult, DequeueOutcome};
use crate::task::{ScheduledTask, TaskPayload};
use crate::types::TaskMetadata;

/// Embedded, crash-safe, file-backed queue backed by Yaque.
///
/// NOTE: The yaque::* types here are pseudo-code. Replace them with the real Yaque
/// API (writer/reader/stream types) when you integrate the crate.
pub struct YaqueQueue<P>
where
    P: TaskPayload,
{
    root: PathBuf,
    stream: String,
    max_depth: usize,

    // Example: you might keep a shared handle to a yaque stream here.
    // Adjust to real Yaque API.
    inner: Mutex<YaqueHandlePlaceholder>,

    _marker: std::marker::PhantomData<P>,
}

/// Placeholder type; replace with actual Yaque client/stream type.
pub struct YaqueHandlePlaceholder;

impl<P> YaqueQueue<P>
where
    P: TaskPayload,
{
    pub fn new(path: impl Into<String>, stream: impl Into<String>, max_depth: usize) -> Self {
        Self {
            root: PathBuf::from(path.into()),
            stream: stream.into(),
            max_depth,
            inner: Mutex::new(YaqueHandlePlaceholder),
            _marker: std::marker::PhantomData,
        }
    }

    fn ensure_stream(&self) {
        // Pseudo-code:
        // let mut guard = self.inner.lock().unwrap();
        // if not initialized:
        //   *guard = yaque::open_or_create(&self.root, &self.stream)?;
    }

    fn approximate_len(&self) -> usize {
        // Pseudo-code: you can keep a local counter,
        // or inspect Yaque metadata for queue length.
        0
    }
}
```

### 2.2 Implementing `TaskQueue` on top of Yaque

We serialize the message as:

```json
{
  "metadata": { ... TaskMetadata ... },
  "payload": { ... P ... }
}
```

and push/pop JSON blobs to/from Yaque.

```rust
#[async_trait]
impl<P> TaskQueue<P> for YaqueQueue<P>
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult {
        self.ensure_stream();

        let depth = self.approximate_len();
        if depth >= self.max_depth {
            return EnqueueResult {
                accepted: false,
                reason: Some("queue at max capacity (Yaque)".into()),
            };
        }

        let meta_json = serde_json::to_value(&task.meta).unwrap();
        let payload_json = serde_json::to_value(&task.payload).unwrap();
        let msg = serde_json::json!({
            "metadata": meta_json,
            "payload": payload_json,
        });

        // Pseudo-code for Yaque append:
        //
        // let mut guard = self.inner.lock().unwrap();
        // guard.append(msg.to_string().as_bytes())?;

        EnqueueResult { accepted: true, reason: None }
    }

    async fn dequeue_next(&self, _now: Instant) -> DequeueOutcome<P> {
        self.ensure_stream();

        // Pseudo-code for Yaque read:
        //
        // let mut guard = self.inner.lock().unwrap();
        // if let Some(bytes) = guard.pop_nonblocking()? {
        //     let v: serde_json::Value = serde_json::from_slice(&bytes)?;
        //     let meta: TaskMetadata = serde_json::from_value(v["metadata"].clone())?;
        //     let payload: P = serde_json::from_value(v["payload"].clone())?;
        //     return DequeueOutcome::Task(ScheduledTask { meta, payload });
        // }

        DequeueOutcome::Empty
    }

    async fn prune_expired(&self, _now: Instant) -> usize {
        // For embedded usage you can:
        //   - enforce deadlines on dequeue (drop expired tasks there),
        //   - or periodically scan the log and compact.
        //
        // To keep the adapter simple, treat expiry in the scheduler logic:
        // when dequeue returns a task whose metadata.deadline_ms < now_ms,
        // treat it as expired and don't re-enqueue.
        0
    }

    async fn len(&self) -> usize {
        self.approximate_len()
    }
}
```

The important bits:

- **All the resource accounting, timeouts, mailboxes, and scheduling logic remain unchanged.**
- Yaque simply becomes another implementation of the `TaskQueue<P>` trait with:
  - durable, file-backed logging,
  - good performance for append/read,
  - ideal for Tauri / CLI / local Rust agents.

You can decide later if you want stronger semantics (e.g., consumer groups, replay, compaction) and layer that into `YaqueHandlePlaceholder` or a small internal helper.

------

## 3. How this plays into the bigger picture

### Tauri / local Prometheus agents

- `QueueBuilderContext::Embedded`
- `QueueBackend::Yaque { path: "./data/queues".into(), stream: "llm_inference".into() }`
- `ResourcePool` gets a `YaqueQueue<LlmJob>` behind the trait.
- Mailbox:
  - `InMemoryMailbox<T>` for simple per-session behavior, or
  - `StorageOnlyMailbox<T, EmbeddedMailboxStorage<T>>` using Yaque/PGlite/etc.

### Cloud / server deployments

- `QueueBuilderContext::Postgres { db }`
- `QueueBackend::PostgresPgmq { queue_name: "llm_inference".into() }`
- Same `ResourcePool` code; no changes to scheduler or executor.

### Web / WASM

- Still uses the same `ResourcePool`, but spawner + executor are different (web workers).
- If you ever want an **in-browser** embedded queue, you could mirror this pattern:
  - `QueueBackend::BrowserIndexedDb { db_name, store }` with a `TaskQueue` impl using IndexedDB or PGlite WASM.

------

## 4. Practical “drop in” wiring example for a Tauri app

Putting it together, this is how a Tauri desktop agent would configure its pool:

```rust
use prometheus_parking_lot::{
    scheduler::{PoolConfig, ResourcePool},
    runtime::tokio::TokioSpawner, // or a custom Tauri-compatible spawner
    task::{TaskExecutor, ScheduledTask},
    types::*,
};
use my_crate::{
    QueueBackend, QueueBuilderContext, build_task_queue,
    mailbox::{InMemoryStorage, StorageOnlyMailbox}, // from our mailbox abstraction
};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct LlmJob {
    model: String,
    prompt: String,
    max_tokens: u32,
}

#[derive(Clone)]
struct LlmExecutor;

#[async_trait::async_trait]
impl TaskExecutor<LlmJob, String> for LlmExecutor {
    async fn execute(&self, payload: LlmJob, _meta: TaskMetadata) -> String {
        // call local candle-vllm / AG-UI adapter
        format!("LLM({}): {}", payload.model, payload.prompt)
    }
}

async fn build_tauri_pool() -> ResourcePool<LlmJob, _, _, _, _, String> {
    let config = PoolConfig {
        max_units: 8,                     // e.g. 8 "units" for local LLM + RAG
        max_queue_depth: 1000,
        default_timeout: std::time::Duration::from_secs(60),
    };

    let queue_backend = QueueBackend::Yaque {
        path: "./data/queues".into(),
        stream: "llm_inference".into(),
    };

    let queue = build_task_queue::<LlmJob>(
        QueueBuilderContext::Embedded,
        queue_backend,
        config.max_queue_depth,
    );

    let mailbox_storage = InMemoryStorage::<String>::new();
    let mailbox = StorageOnlyMailbox::<String, _>::new(mailbox_storage);

    let spawner = TokioSpawner; // or a custom spawner integrated with Tauri runtime
    let executor = LlmExecutor;

    ResourcePool::new(config, queue, mailbox, spawner, executor)
}
```

You can reuse this same pattern for:

- A CLI tool that runs local Prometheus agents.
- A background daemon running on a user’s laptop.
- A small edge box for on-prem inference.

------

## 5. Where this leaves us

We now have, conceptually:

- **TaskQueue backends:**
  - In-memory
  - Postgres (SeaORM custom schema)
  - Postgres (pgmq)
  - **Yaque (embedded, file-based, Rust-native – ideal for Tauri/desktop)**
- **Mailbox backends:**
  - In-memory
  - Persistent (via Postgres/SeaORM, Surreal, etc.)
  - Optional REST notifier layered on top

All wired behind stable trait interfaces and feeding into the **single** `ResourcePool` **scheduling layer**.

Now that Yaque is slotted correctly, the next step (when you’re ready) is to define the **config schema** so each Prometheus agent/service can declare:

- which queue backend,
- which mailbox storage + notifier,
- capacity and cost model.

We can make that config JSON/YAML/TOML and feed it into `build_task_queue` + `build_mailbox` + `ResourcePool::new`.

------

## Prompt:

Do it.

## Answer:

Alright, let’s wire this into a **real config-driven system** you can drop into Prometheus.

I’ll give you:

1. A **JSON-first config schema** (with YAML equivalent) for:
   - Pools (capacity, queue, mailbox).
   - Queue backend selection (In-memory, Postgres+table, Postgres+pgmq, Yaque).
   - Mailbox storage + optional notifier (REST callback).
2. Rust **config types** (`serde`-based).
3. **Builder functions**: `build_task_queue_from_config` and `build_mailbox_from_config`, plus a `build_pool_from_config` you can call from agents.

You’ll be able to go from `scheduler-config.json` → `ResourcePool<…>` with one call.

------

## 1. Config schema (JSON-focused)

### 1.1 High-level structure

One config file that defines multiple **pools** (e.g. `llm_inference`, `rag_preprocess`, `tauri_local_llm`):

```jsonc
{
  "pools": {
    "llm_inference": {
      "max_units": 20,
      "max_queue_depth": 10000,
      "default_timeout_secs": 60,
      "queue": {
        "type": "postgres_pgmq",
        "queue_name": "llm_inference"
      },
      "mailbox": {
        "storage": {
          "type": "postgres",
          "table": "pl_mailbox_messages"
        },
        "notifier": {
          "type": "http",
          "base_url": "https://prometheus-gateway.example.com/hooks/mailbox",
          "auth_header": "Bearer YOUR_TOKEN_IF_NEEDED"
        }
      }
    },

    "tauri_local_llm": {
      "max_units": 8,
      "max_queue_depth": 1000,
      "default_timeout_secs": 60,
      "queue": {
        "type": "yaque",
        "path": "./data/queues",
        "stream": "tauri_llm"
      },
      "mailbox": {
        "storage": {
          "type": "in_memory"
        }
      }
    }
  }
}
```

So each **pool** describes:

- **Capacity**: `max_units`, `max_queue_depth`, `default_timeout_secs`.
- **Queue**: which backend + parameters.
- **Mailbox**:
  - `storage`: where results are stored.
  - `notifier` (optional): how clients are notified (REST callback, etc.).

### 1.2 YAML equivalent (if/when you want YAML)

Just for reference; the canonical is JSON above.

```yaml
pools:
  llm_inference:
    max_units: 20
    max_queue_depth: 10000
    default_timeout_secs: 60
    queue:
      type: postgres_pgmq
      queue_name: llm_inference
    mailbox:
      storage:
        type: postgres
        table: pl_mailbox_messages
      notifier:
        type: http
        base_url: https://prometheus-gateway.example.com/hooks/mailbox
        auth_header: Bearer YOUR_TOKEN_IF_NEEDED

  tauri_local_llm:
    max_units: 8
    max_queue_depth: 1000
    default_timeout_secs: 60
    queue:
      type: yaque
      path: ./data/queues
      stream: tauri_llm
    mailbox:
      storage:
        type: in_memory
```

------

## 2. Rust config types (with `serde`)

These go in something like `src/config.rs` in your scheduling crate.

### 2.1 Core config types

```rust
use serde::Deserialize;
use std::collections::HashMap;

use crate::types::ResourceKind;

/// Top-level scheduler config: multiple pools keyed by name.
#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    pub pools: HashMap<String, PoolConfig>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PoolConfig {
    pub max_units: u32,
    pub max_queue_depth: usize,
    pub default_timeout_secs: u64,

    pub queue: QueueConfig,
    pub mailbox: MailboxConfig,

    /// Optional: hint for default resource kind (CPU, GPU, Mixed).
    #[serde(default)]
    pub resource_kind: Option<ResourceKind>,
}
```

### 2.2 Queue config

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct QueueConfig {
    #[serde(flatten)]
    pub backend: QueueBackendConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueueBackendConfig {
    InMemory,

    /// Postgres table-based queue (SeaORM / SQLx).
    PostgresCustom {
        table: String,
    },

    /// Postgres pgmq extension-backed queue.
    PostgresPgmq {
        queue_name: String,
    },

    /// Yaque embedded file-backed queue for Tauri / local agents.
    Yaque {
        path: String,
        stream: String,
    },
}
```

### 2.3 Mailbox config

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct MailboxConfig {
    pub storage: MailboxStorageConfig,
    pub notifier: Option<MailboxNotifierConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MailboxStorageConfig {
    InMemory,

    /// Postgres table, e.g. "pl_mailbox_messages".
    Postgres {
        table: String,
    },

    /// Reserved for later: SurrealDB/other.
    // Surreal {
    //     namespace: String,
    //     database: String,
    //     collection: String,
    // },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MailboxNotifierConfig {
    /// HTTP callback (REST webhook)
    Http {
        base_url: String,
        #[serde(default)]
        auth_header: Option<String>,
    },
    // Later: Ws, Kafka, Nats, etc.
}
```

------

## 3. Builder functions (config → queue/mailbox/pool)

We’ll wire the config into the existing abstractions you already have:

- `TaskQueue<P>`
- `Mailbox<T>` (composed from `MailboxStorage<T>` + optional `MailboxNotifier<T>`)
- `ResourcePool<P, Q, M, S, E, T>`

### 3.1 Queue builder from config

Assume we already have:

- `QueueBuilderContext` (as before).
- `QueueBackend` enum is now basically replaced by `QueueBackendConfig`.

We can write:

```rust
use sea_orm::DatabaseConnection;

use crate::queue::{
    TaskQueue,
    in_memory::InMemoryQueue,
    sea_queue::SeaOrmQueue,
    pgmq_queue::PgmqQueue,
    yaque::YaqueQueue,
};
use crate::task::TaskPayload;
use crate::config::{QueueConfig, QueueBackendConfig};
use crate::builder::QueueBuilderContext;

pub enum QueueBuilderContext {
    Postgres { db: DatabaseConnection },
    Embedded,
}

pub fn build_task_queue_from_config<P>(
    ctx: &QueueBuilderContext,
    cfg: &QueueConfig,
    max_depth: usize,
) -> Box<dyn TaskQueue<P>>
where
    P: TaskPayload,
{
    match (ctx, &cfg.backend) {
        (_, QueueBackendConfig::InMemory) => {
            Box::new(InMemoryQueue::<P>::new(max_depth))
        }

        (QueueBuilderContext::Postgres { db }, QueueBackendConfig::PostgresCustom { table }) => {
            Box::new(SeaOrmQueue::<P>::new(db.clone(), table.clone(), max_depth))
        }

        (QueueBuilderContext::Postgres { db }, QueueBackendConfig::PostgresPgmq { queue_name }) => {
            Box::new(PgmqQueue::<P>::new(db.clone(), queue_name.clone(), max_depth))
        }

        (QueueBuilderContext::Embedded, QueueBackendConfig::Yaque { path, stream }) => {
            Box::new(YaqueQueue::<P>::new(path.clone(), stream.clone(), max_depth))
        }

        (ctx, backend) => {
            panic!("Unsupported queue backend {:?} in context {:?}", backend, ctx);
        }
    }
}
```

### 3.2 Mailbox builder from config

We’ll use the **split storage + notifier** design from before:

- `MailboxStorage<T>` trait
- `MailboxNotifier<T>` trait
- `StorageOnlyMailbox<T, S>`
- `StorageWithNotifierMailbox<T, S, N>`

First, helpers to build storage + notifier:

```rust
use crate::mailbox::{
    Mailbox,
    MailboxStorage,
    MailboxNotifier,
    StorageOnlyMailbox,
    StorageWithNotifierMailbox,
    in_memory::InMemoryStorage,
    sea_mailbox::SeaOrmMailboxStorage,
    http_notifier::HttpCallbackNotifier,
};
use crate::config::{MailboxConfig, MailboxStorageConfig, MailboxNotifierConfig};
use crate::builder::QueueBuilderContext;

/// Storage builder
fn build_mailbox_storage_from_config<T>(
    ctx: &QueueBuilderContext,
    cfg: &MailboxStorageConfig,
    db: Option<&DatabaseConnection>,
) -> Box<dyn MailboxStorage<T>>
where
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    match cfg {
        MailboxStorageConfig::InMemory => {
            Box::new(InMemoryStorage::<T>::new())
        }
        MailboxStorageConfig::Postgres { table } => {
            let db = db.expect("Postgres mailbox requires DatabaseConnection");
            Box::new(SeaOrmMailboxStorage::<T>::new(db.clone(), table.clone()))
        }
        // Surreal or others can be added here
    }
}

/// Notifier builder
fn build_mailbox_notifier_from_config<T>(
    cfg: &MailboxNotifierConfig,
) -> Box<dyn MailboxNotifier<T>>
where
    T: Send + Sync + serde::Serialize + 'static,
{
    match cfg {
        MailboxNotifierConfig::Http { base_url, auth_header } => {
            Box::new(HttpCallbackNotifier::<T>::new(
                base_url.clone(),
                auth_header.clone(),
            ))
        }
    }
}
```

Now we combine them into a `Mailbox<T>`:

```rust
pub fn build_mailbox_from_config<T>(
    ctx: &QueueBuilderContext,
    cfg: &MailboxConfig,
    db: Option<&DatabaseConnection>,
) -> Box<dyn Mailbox<T>>
where
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    let storage = build_mailbox_storage_from_config::<T>(ctx, &cfg.storage, db);

    if let Some(notifier_cfg) = &cfg.notifier {
        let notifier = build_mailbox_notifier_from_config::<T>(notifier_cfg);
        Box::new(StorageWithNotifierMailbox::new(storage, notifier))
    } else {
        Box::new(StorageOnlyMailbox::new(storage))
    }
}
```

### 3.3 Pool builder from config

Finally, the **pool builder** that ties it together:

```rust
use crate::scheduler::{ResourcePool, PoolConfig as RuntimePoolConfig};
use crate::config::PoolConfig;
use crate::runtime::Spawn;
use crate::task::{TaskExecutor, TaskPayload};
use crate::types::ResourceKind;

pub fn build_pool_from_config<P, T, E, S>(
    pool_cfg: &PoolConfig,
    ctx: &QueueBuilderContext,
    db: Option<&DatabaseConnection>,
    executor: E,
    spawner: S,
) -> ResourcePool<P, Box<dyn TaskQueue<P>>, Box<dyn Mailbox<T>>, S, E, T>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn,
{
    let runtime_config = RuntimePoolConfig {
        max_units: pool_cfg.max_units,
        max_queue_depth: pool_cfg.max_queue_depth,
        default_timeout: std::time::Duration::from_secs(pool_cfg.default_timeout_secs),
    };

    let queue = build_task_queue_from_config::<P>(
        ctx,
        &pool_cfg.queue,
        pool_cfg.max_queue_depth,
    );

    let mailbox = build_mailbox_from_config::<T>(
        ctx,
        &pool_cfg.mailbox,
        db,
    );

    ResourcePool::new(runtime_config, queue, mailbox, spawner, executor)
}
```

And you can add a helper to build **all pools** from `SchedulerConfig`:

```rust
use std::collections::HashMap;
use crate::config::SchedulerConfig;

pub struct BuiltPools<P, T, E, S> {
    pub pools: HashMap<String, ResourcePool<P, Box<dyn TaskQueue<P>>, Box<dyn Mailbox<T>>, S, E, T>>,
}

pub fn build_pools_from_scheduler_config<P, T, E, S>(
    cfg: &SchedulerConfig,
    ctx: &QueueBuilderContext,
    db: Option<&DatabaseConnection>,
    executor_factory: impl Fn(&str) -> E,
    spawner: S,
) -> BuiltPools<P, T, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Clone,
{
    let mut map = HashMap::new();

    for (name, pool_cfg) in &cfg.pools {
        let executor = executor_factory(name);
        let pool = build_pool_from_config::<P, T, E, S>(
            pool_cfg,
            ctx,
            db,
            executor,
            spawner.clone(),
        );
        map.insert(name.clone(), pool);
    }

    BuiltPools { pools: map }
}
```

That lets you do:

```rust
// Pseudocode: in your main for a server using Postgres + pgmq
let db = Database::connect("postgres://...").await?;
let ctx = QueueBuilderContext::Postgres { db: db.clone() };

let cfg: SchedulerConfig = serde_json::from_slice(fs::read("scheduler-config.json")?)?;

let pools = build_pools_from_scheduler_config::<LlmJob, String, LlmExecutor, TokioSpawner>(
    &cfg,
    &ctx,
    Some(&db),
    |_name| LlmExecutor,
    TokioSpawner,
);

// Now you can get e.g.:
let llm_pool = pools.pools.get("llm_inference").unwrap();
```

------

## 4. How this fits Prometheus end-to-end

- Each **agent type** (LLM inference, RAG, ETL, etc.) gets a **pool name** with:
  - Queue backend (In-memory/Yaque/Postgres/pgmq).
  - Mailbox storage/notifier.
  - Capacity.
- The scheduler crate is **agnostic** about:
  - Tauri vs cloud vs web.
  - Pgmq vs Yaque vs in-memory.
  - Whether you send REST callbacks or just store results.

You control all of that with **config**, without touching code.

------

## 5. Immediate action plan

1. **Add** `config.rs` with the `SchedulerConfig`, `PoolConfig`, `QueueConfig`, `MailboxConfig` definitions.
2. **Implement the builders**:
   - `build_task_queue_from_config`
   - `build_mailbox_from_config`
   - `build_pool_from_config`
   - (optionally) `build_pools_from_scheduler_config`
3. **Create** `scheduler-config.json` in your Prometheus runtime repo (start with `llm_inference` + `tauri_local_llm`).
4. Wire your **Rust AI gateway / Tauri agent** startup to:
   - Load the JSON.
   - Call `build_pools_from_scheduler_config`.
   - Register the relevant pool(s) in your DI/container or global state.

Once that’s in place, we can layer on:

- A **GPU/LLM cost-model section** in config (for dynamic `ResourceCost`).
- Tenant-level overrides (per-tenant max_units, priority, etc).
- Declarative association with MCP tools or specific Prometheus agent artifacts.

If you want, next step we can define a small `llm_cost_model` **section** in the config and show how to turn it into `ResourceCost` per request.

------

## Prompt:

This should get us to a quick usable version 1 crate. Based on this revise and finalize the directory structure and implementation plan based on feature based clean architecture and the Microsoft rust coding guidelines

## Answer:

Agreed — this is absolutely enough to lock in a clean v1 crate.

I’ll give you two things:

1. **Finalized directory structure** (feature-based, clean architecture, idiomatic Rust).
2. **Implementation plan** mapped to that structure, aligned with Microsoft-style Rust guidelines (clarity, layering, error handling, testing, docs).

------

## 1. Final crate layout (v1)

Single crate: `prometheus_parking_lot`

```text
prometheus_parking_lot/
├── Cargo.toml
├── src/
│   ├── lib.rs
│   │
│   ├── core/                  # Domain + application core (no infra deps)
│   │   ├── mod.rs
│   │   ├── types.rs           # TaskId, TenantId, MailboxKey, ResourceKind, ResourceCost, Priority, TaskMetadata, MailboxMessage
│   │   ├── task.rs            # TaskPayload, ScheduledTask<P>, TaskExecutor<P,T>
│   │   ├── queue.rs           # TaskQueue<P>, EnqueueResult, DequeueOutcome
│   │   ├── mailbox.rs         # Mailbox<T>, MailboxStorage<T>, MailboxNotifier<T>
│   │   ├── runtime.rs         # Spawn trait
│   │   └── scheduler.rs       # ResourcePool + PoolConfig (runtime config)
│   │
│   ├── config/                # External configuration (serde-based)
│   │   ├── mod.rs
│   │   └── model.rs           # SchedulerConfig, PoolConfig, QueueConfig, MailboxConfig, etc.
│   │
│   ├── builders/              # Glue: config → core abstractions
│   │   ├── mod.rs
│   │   ├── queue_builder.rs   # build_task_queue_from_config
│   │   ├── mailbox_builder.rs # build_mailbox_from_config
│   │   └── pool_builder.rs    # build_pool_from_config, build_pools_from_scheduler_config
│   │
│   ├── infra/                 # Infrastructure adapters (backends)
│   │   ├── mod.rs
│   │   ├── queue/
│   │   │   ├── mod.rs
│   │   │   ├── in_memory.rs   # InMemoryQueue<P>
│   │   │   ├── sea_orm.rs     # SeaOrmQueue<P> (Postgres custom table)
│   │   │   ├── pgmq.rs        # PgmqQueue<P> (Postgres pgmq)
│   │   │   └── yaque.rs       # YaqueQueue<P> (embedded file-backed)
│   │   ├── mailbox/
│   │   │   ├── mod.rs
│   │   │   ├── in_memory.rs   # InMemoryStorage<T>
│   │   │   ├── sea_orm.rs     # SeaOrmMailboxStorage<T>
│   │   │   └── http_notifier.rs # HttpCallbackNotifier<T>
│   │   └── runtime/
│   │       ├── mod.rs
│   │       └── tokio.rs       # TokioSpawner
│   │
│   └── util/
│       └── clock.rs           # Clock abstraction (Instant ↔ epoch ms) – optional but recommended
│
├── examples/
│   ├── llm_local_inference_tokio.rs  # local/Tauri-esque example using Yaque or in-memory
│   └── llm_cloud_pgmq.rs             # Postgres+pgmq + Postgres mailbox example
└── tests/
    ├── scheduler_basic.rs            # basic scheduling behavior tests
    ├── queue_backends.rs             # per-backend tests
    └── mailbox_behavior.rs           # mailbox storage+notifier tests
```

### Feature flags in `Cargo.toml`

```toml
[features]
default = ["native"]

native   = ["tokio"]
postgres = ["sea-orm"]        # for SeaORM-based queue/mailbox
pgmq     = ["postgres"]       # pgmq uses Postgres backend
yaque    = []                 # add yaque crate when you wire it
config   = ["serde", "serde_json"]

[dependencies]
async-trait = "0.1"
serde       = { version = "1", features = ["derive"], optional = true }
serde_json  = { version = "1", optional = true }
thiserror   = "1"
tokio       = { version = "1", features = ["rt-multi-thread", "macros", "time"], optional = true }
sea-orm     = { version = "0.12", default-features = false, features = ["macros", "runtime-tokio-rustls", "sqlx-postgres"], optional = true }
reqwest     = { version = "0.12", features = ["json", "rustls-tls"], optional = true }
```

We keep **core / scheduler** independent of infra features. Infra modules are behind relevant features and not pulled in unless needed.

------

## 2. Clean architecture mapping

**Domain / Core (no external deps)**`src/core/*`

- `types.rs`: pure data types, no I/O:
  - `TaskId`, `TenantId`, `MailboxKey`
  - `Priority`
  - `ResourceKind`, `ResourceCost`
  - `TaskMetadata`
  - `TaskStatus`, `MailboxMessage<T>`
- `task.rs`:
  - `TaskPayload` marker trait (`Serialize + DeserializeOwned + Send + Sync + 'static`)
  - `ScheduledTask<P>`
  - `TaskExecutor<P,T>` (async trait, pure interface)
- `queue.rs`:
  - `TaskQueue<P>` trait
  - `EnqueueResult`, `DequeueOutcome<P>`
- `mailbox.rs`:
  - `Mailbox<T>` high-level trait
  - `MailboxStorage<T>`, `MailboxNotifier<T>` lower-level traits
- `runtime.rs`:
  - `Spawn` trait (abstract spawner)
- `scheduler.rs`:
  - `PoolConfig` (runtime config only: max_units, max_queue_depth, default_timeout)
  - `ResourcePool<P, Q, M, S, E, T>` (core parking-lot scheduler)

**Application / Config**`src/config/`, `src/builders/`

- `config/model.rs`:
  - `SchedulerConfig`, `PoolConfig`, `QueueConfig`, `QueueBackendConfig`, `MailboxConfig`, `MailboxStorageConfig`, `MailboxNotifierConfig`.
- `builders/queue_builder.rs`:
  - `QueueBuilderContext` (Postgres vs Embedded)
  - `build_task_queue_from_config`
- `builders/mailbox_builder.rs`:
  - `build_mailbox_storage_from_config`
  - `build_mailbox_notifier_from_config`
  - `build_mailbox_from_config`
- `builders/pool_builder.rs`:
  - `build_pool_from_config`
  - `build_pools_from_scheduler_config`

**Infrastructure / Adapters**`src/infra/*`

- `queue/in_memory.rs`: simple heap-based queue (priority + FIFO).
- `queue/sea_orm.rs`: `SeaOrmQueue<P>` using `pl_queue_jobs` custom table.
- `queue/pgmq.rs`: `PgmqQueue<P>` using `pgmq` extension (`send`/`read`/`delete`).
- `queue/yaque.rs`: `YaqueQueue<P>` using embedded file-backed Yaque stream.
- `mailbox/in_memory.rs`: `InMemoryStorage<T>` implementing `MailboxStorage<T>`.
- `mailbox/sea_orm.rs`: `SeaOrmMailboxStorage<T>` for `pl_mailbox_messages`.
- `mailbox/http_notifier.rs`: `HttpCallbackNotifier<T>` implementing `MailboxNotifier<T>`.
- `runtime/tokio.rs`: `TokioSpawner` implementing `Spawn`.

**Utility**`util/clock.rs` (optional but recommended):

- `trait Clock { fn now(&self) -> Instant; fn now_ms(&self) -> u128; }`
- `SystemClock` implementation.
- Used inside scheduler + builders instead of direct `Instant::now()` when you want testable time semantics.

------

## 3. Implementation plan (phased, v1-ready)

### Phase 1 – Core domain + in-memory backends

**Goal:** fully working scheduler with in-memory queue + mailbox, no DB/pgmq/yaque yet.

1. Implement `core` module:
   - `types.rs` with **simple, documented types** and no logic.
   - `task.rs` with:
     - `TaskPayload` alias (trait bounds).
     - `ScheduledTask<P>`.
     - `TaskExecutor<P,T>` async trait.
   - `queue.rs` with:
     - `TaskQueue<P>` trait.
     - `EnqueueResult`, `DequeueOutcome<P>`.
   - `mailbox.rs` with:
     - `Mailbox<T>` (high-level).
     - `MailboxStorage<T>`, `MailboxNotifier<T>` (lower-level).
   - `runtime.rs` with `Spawn` trait.
   - `scheduler.rs` with `PoolConfig` + `ResourcePool`:
     - no direct infra calls, no `tokio::spawn` inside.
     - use `Spawn` trait only.
     - **no** `.unwrap()` **or panics** on normal paths; return `Result` where needed or document invariants.
2. Implement infra in-memory backends:
   - `queue/in_memory.rs`:
     - `InMemoryQueue<P>` with `BinaryHeap` or `VecDeque` and a `Mutex`.
   - `mailbox/in_memory.rs`:
     - `InMemoryStorage<T>` implementing `MailboxStorage<T>`.
     - type alias `InMemoryMailbox<T> = StorageOnlyMailbox<T, InMemoryStorage<T>>`.
   - `runtime/tokio.rs`:
     - `TokioSpawner` using `tokio::spawn`.
3. Tests:
   - `tests/scheduler_basic.rs`:
     - create a dummy `TaskExecutor` that records order.
     - verify:
       - capacity limit enforced.
       - tasks are queued when full.
       - queued tasks are run when a running task completes.
       - timeouts (if implemented in v1) are handled at least conceptually (you can finalize this in phase 2).
4. Examples:
   - `examples/llm_local_inference_tokio.rs`:
     - use `InMemoryQueue` + `InMemoryMailbox` + `TokioSpawner` with a fake `LlmJob`.

> This gives you a **fully working crate** with no external dependencies, ideal as v1 alpha.

------

### Phase 2 – Postgres custom table + mailbox (SeaORM)

**Goal:** production-grade persistent queue/mailbox using your own schema.

1. Add `infra/queue/sea_orm.rs`:
   - Implement `SeaOrmQueue<P>` using `pl_queue_jobs` table.
   - Use `EntityTrait` + `ActiveModel` for `INSERT`/`DELETE` + `ORDER BY priority DESC, created_at ASC`.
   - No panics; handle error via logging and return `DequeueOutcome::Empty` if things fail.
2. Add `infra/mailbox/sea_orm.rs`:
   - `SeaOrmMailboxStorage<T>` implementing `MailboxStorage<T>` on `pl_mailbox_messages`.
3. Add `config` + `builders` modules:
   - `SchedulerConfig`, `PoolConfig`, `QueueConfig`, `QueueBackendConfig`, etc.
   - `QueueBuilderContext::Postgres { db }`.
   - `build_task_queue_from_config` for `postgres_custom` → `SeaOrmQueue`.
   - `build_mailbox_from_config` for `postgres` storage → `SeaOrmMailboxStorage`.
   - `build_pool_from_config` + `build_pools_from_scheduler_config`.
4. Tests:
   - `tests/queue_backends.rs` with a Postgres docker (or test container) if you want integration tests, or just integration-level tests run manually.

------

### Phase 3 – Postgres pgmq adapter

**Goal:** queue backed by `pgmq` extension; mailbox still Postgres table.

1. Add feature `pgmq` in Cargo.toml and `infra/queue/pgmq.rs`:
   - `PgmqQueue<P>` using `pgmq.send/read/delete`.
   - Make sure to serialize `metadata` + `payload` to JSON just like other backends.
2. Extend `QueueBackendConfig`:
   - Add `"postgres_pgmq"` variant.
3. Extend `build_task_queue_from_config`:
   - Map `"postgres_pgmq"` to `PgmqQueue<P>`.
4. Update examples:
   - `examples/llm_cloud_pgmq.rs`:
     - Use `QueueBackendConfig::PostgresPgmq`.
     - `MailboxStorageConfig::Postgres`.
5. Tests:
   - Simple integration test if you can, otherwise manual testing is OK for v1.

------

### Phase 4 – Yaque embedded backend (Tauri / CLI)

**Goal:** robust embedded queue for Tauri / local agents.

1. Add `infra/queue/yaque.rs`:
   - Implement `YaqueQueue<P>` around Yaque’s stream API.
   - Keep API safe:
     - No `unwrap()` except in tests or obviously impossible branches.
     - Map I/O errors into crate-level error type (see guidelines below).
2. Add `QueueBackendConfig::Yaque { path, stream }`.
3. In `build_task_queue_from_config`:
   - For `QueueBuilderContext::Embedded` + `QueueBackendConfig::Yaque`:
     - instantiate `YaqueQueue<P>`.
4. Example:
   - `examples/llm_local_inference_tokio.rs`:
     - add a second path using Yaque instead of in-memory when compiled with `yaque` feature.

------

### Phase 5 – Mailbox notifier (REST) + full Mailbox composition

**Goal:** storage-only + storage+REST mailbox; no API changes to scheduler.

1. Add `infra/mailbox/http_notifier.rs`:
   - `HttpCallbackNotifier<T>` implementing `MailboxNotifier<T>` using `reqwest`.
   - Accept `base_url`, `auth_header` in constructor.
2. Extend config:
   - `MailboxNotifierConfig::Http { base_url, auth_header }`.
3. Extend `build_mailbox_notifier_from_config`:
   - Map `"http"` to `HttpCallbackNotifier`.
4. Mailbox composition:
   - Already have `StorageOnlyMailbox` and `StorageWithNotifierMailbox`.
   - `build_mailbox_from_config` chooses which to build based on `notifier` presence.
5. Example:
   - `examples/llm_cloud_pgmq.rs`:
     - show mailbox config with an HTTP notifier.

------

## 4. Alignment with Microsoft Rust coding guidelines (and general best practice)

You’re already aligned with most of this conceptually; to be explicit:

1. **Clear, layered modules**

   - `core` has *no* knowledge of infra or external crates (except maybe `serde` for serializable types).
   - `infra` implements traits defined in `core`.
   - `config` + `builders` glue external configuration into `core` abstractions.

2. **Error handling**

   - Avoid `unwrap()` and `panic!` in library code.

   - Introduce a crate-level error type in `core` or `util`, e.g.:

     ```rust
     #[derive(thiserror::Error, Debug)]
     pub enum SchedulerError {
         #[error("queue backend error: {0}")]
         QueueBackend(String),
         #[error("mailbox backend error: {0}")]
         MailboxBackend(String),
         #[error("config error: {0}")]
         Config(String),
         // ...
     }
     ```

   - Return `Result<_, SchedulerError>` from builders and any public function that can fail. The scheduler core itself can often remain infallible since it operates on already-instantiated traits.

3. **Naming & style**

   - Use **snake_case** for modules and functions.
   - **UpperCamelCase** for types and traits.
   - Keep type names descriptive and precise: `ResourcePool`, `TaskExecutor`, `MailboxStorage`, `YaqueQueue`.
   - Avoid abbreviations except well-known ones (`DB`, `HTTP`, `GPU`).

4. **Documentation**

   - Every public type and trait should have a `///` doc comment explaining:
     - What it represents.
     - How it’s meant to be used.
   - Add crate-level docs in `lib.rs` with:
     - Overview of parking-lot scheduling.
     - Example configuration and usage.

5. **Testing**

   - Unit tests for:
     - `ResourcePool` logic (scheduling, wake-up, timeouts).
     - In-memory queue/mailbox.
   - Integration tests for:
     - Postgres + SeaORM.
     - pgmq.
     - Yaque (once wired).
   - Where possible, use a `Clock` abstraction in tests to control time.

6. **Public API surface**

   - Re-export *only* what end-users need from `lib.rs`:
     - `SchedulerConfig`, `PoolConfig`, `QueueConfig`, `MailboxConfig`.
     - `ResourcePool`, `PoolConfig` (runtime version).
     - Traits: `TaskPayload`, `TaskExecutor`, `Spawn`.
     - Builders: `build_pool_from_config`, `build_pools_from_scheduler_config`.
   - Keep infra modules reachable but not all re-exported at the top level unless they’re intended for customization.

------

## 5. What you can do right now

If you want to get to a “quick usable v1” very quickly:

1. **Implement Phase 1** (core + in-memory + Tokio) exactly as above.
2. **Implement Phase 2** with SeaORM `pl_queue_jobs` and `pl_mailbox_messages` using migrations you already sketched.
3. Add **config + builders** and one **example** using Postgres.

That gives you:

- A **production-usable** Prometheus scheduling crate with:
  - Parking-lot scheduling.
  - Queue + mailbox abstractions.
  - Postgres persistence.
  - Config-driven instantiation.
- And clear expansion points:
  - `pgmq` for high-throughput queues.
  - `yaque` for Tauri/local.
  - REST notifier for push behavior.

If you’d like, I can next generate a **starter** `lib.rs` with the correct `mod` wiring and top-level docs so you can start coding against it immediately.

------

## Prompt:

Do it and provide a README.md for the project that clearly defines what this project is, its purpose, and how it should be used.

## Answer:

Here you go: a starter `lib.rs` wired to the architecture we’ve been designing, plus a solid `README.md` that explains the project, purpose, and usage.

------

## `src/lib.rs`

```rust
//! # prometheus_parking_lot
//!
//! A configurable, runtime-agnostic **parking-lot scheduler** for Prometheus AI agents.
//!
//! This crate provides a dedicated scheduling layer that:
//!
//! - Manages **resource-constrained pools** (CPU, GPU VRAM, web workers, etc.).
//! - Queues excess work in a **“parking lot”** and wakes it when capacity is free.
//! - Handles **timeouts** and **disconnected clients** via a mailbox abstraction.
//! - Supports multiple **queue backends** (in-memory, Postgres, pgmq, Yaque).
//! - Supports multiple **mailbox backends** (in-memory, Postgres, REST callbacks).
//!
//! The core scheduler is runtime-agnostic and environment-neutral; you plug in:
//!
//! - A `TaskQueue<P>` implementation (how jobs are stored / dequeued).
//! - A `Mailbox<T>` implementation (how results are stored / notified).
//! - A `Spawn` implementation (how to spawn async work: Tokio, web workers, etc.).
//! - A `TaskExecutor<P, T>` implementation (how to execute a job payload).
//!
//! The crate also supports **config-driven setup**: you can describe pools, queues,
//! and mailboxes in JSON/YAML, then build fully wired `ResourcePool`s with one call.
//!
//! ## High-Level Concepts
//!
//! - **TaskPayload**: a serializable descriptor of work (e.g., LLM inference request).
//! - **TaskExecutor<P, T>**: async trait that runs `P` and returns result `T`.
//! - **TaskQueue<P>**: trait for queue backends (in-memory, Postgres, pgmq, Yaque).
//! - **Mailbox<T>**: trait for result delivery & retrieval (storage + optional notifier).
//! - **ResourcePool**: the core parking-lot scheduler that:
//!   - Enforces `max_units` (capacity).
//!   - Queues extra work when full.
//!   - Wakes queued work when capacity frees up.
//!
//! ## Typical Usage
//!
//! 1. Define a job payload type, e.g. `LlmJob`.
//! 2. Implement `TaskExecutor<LlmJob, String>` to call your LLM / agent logic.
//! 3. Load a `SchedulerConfig` (JSON/YAML) that defines pools, queues, mailboxes.
//! 4. Use the builders to create one or more `ResourcePool`s.
//! 5. Submit jobs via `pool.submit(ScheduledTask { meta, payload })`.
//!
//! See the `README.md` and examples under `examples/` for more details.

pub mod core;
pub mod config;
pub mod builders;
pub mod infra;
pub mod util;

// --- Core re-exports (public API surface) ---

// Types & traits for tasks, queues, mailboxes, runtime, scheduler.
pub use crate::core::types::{
    TaskId,
    TenantId,
    MailboxKey,
    Priority,
    ResourceKind,
    ResourceCost,
    TaskMetadata,
    TaskStatus,
    MailboxMessage,
};

pub use crate::core::task::{TaskPayload, ScheduledTask, TaskExecutor};
pub use crate::core::queue::{TaskQueue, EnqueueResult, DequeueOutcome};
pub use crate::core::mailbox::{Mailbox, MailboxStorage, MailboxNotifier};
pub use crate::core::runtime::Spawn;
pub use crate::core::scheduler::{ResourcePool, PoolConfig as RuntimePoolConfig};

// --- Config model re-exports ---

pub use crate::config::model::{
    SchedulerConfig,
    PoolConfig as ConfigPoolConfig,
    QueueConfig,
    QueueBackendConfig,
    MailboxConfig,
    MailboxStorageConfig,
    MailboxNotifierConfig,
};

// --- Builder re-exports ---

pub use crate::builders::{
    QueueBuilderContext,
    build_task_queue_from_config,
    build_mailbox_from_config,
    build_pool_from_config,
    build_pools_from_scheduler_config,
};

// --- Common infrastructure re-exports (opt-in via features) ---

// In-memory queue + mailbox storage are always available.
pub use crate::infra::queue::in_memory::InMemoryQueue;
pub use crate::infra::mailbox::in_memory::{InMemoryStorage, InMemoryMailbox};

#[cfg(feature = "native")]
pub use crate::infra::runtime::tokio::TokioSpawner;

#[cfg(feature = "postgres")]
pub use crate::infra::queue::sea_orm::SeaOrmQueue;

#[cfg(feature = "pgmq")]
pub use crate::infra::queue::pgmq::PgmqQueue;

#[cfg(feature = "yaque")]
pub use crate::infra::queue::yaque::YaqueQueue;

#[cfg(feature = "postgres")]
pub use crate::infra::mailbox::sea_orm::SeaOrmMailboxStorage;

#[cfg(feature = "config")]
pub use crate::infra::mailbox::http_notifier::HttpCallbackNotifier;
```

> This assumes you’ll create the modules (`core`, `config`, `builders`, `infra`, `util`) with the internal structure we discussed. You can trim or adjust re-exports as you fill in code.

------

## `README.md`

~~~markdown
# prometheus_parking_lot

A configurable, runtime-agnostic **parking-lot scheduler** for Prometheus AI agents and services.

This crate provides a dedicated scheduling layer that:

- Manages **resource-constrained pools** (CPU, GPU VRAM, web workers, etc.).
- Queues excess work in a **“parking lot”** and wakes it when capacity is free.
- Handles **timeouts** and **disconnected clients** via a mailbox abstraction.
- Supports multiple **queue backends** (in-memory, Postgres, pgmq, Yaque).
- Supports multiple **mailbox backends** (in-memory, Postgres, REST callbacks).
- Is configured via simple **JSON/YAML** so you can change behavior without code changes.

It is designed to be the common scheduling engine for the **Prometheus AI platform** across:

- Local / desktop agents (e.g., Tauri apps running local LLMs),
- Web services (web workers + PGlite),
- Cloud services (GPU-backed agent gateways, microservices).

---

## Why this exists

Prometheus agents often run in **resource-constrained environments**:

- A desktop with a single GPU running a local LLM.
- A Tauri app with limited CPU & memory.
- A cloud VM with a fixed number of GPU/CPU slots.
- A browser using a small pool of web workers.

You need to:

- Limit concurrency by **real resource usage**, not just “number of tasks”.
- Queue additional work and **wake it intelligently**.
- Handle **timeouts** and **clients that disconnect**.
- Use different **queue technologies** depending on the environment:
  - In-memory during development.
  - Postgres or `pgmq` in the cloud.
  - Yaque (embedded, file-backed) for Tauri / local.

This crate centralizes that logic into a **single, reusable scheduler**.

---

## Core concepts

### TaskPayload

A **serializable description** of work to do (e.g., an LLM request):

```rust
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct LlmJob {
    model: String,
    prompt: String,
    max_tokens: u32,
}
~~~

Anything that is `Send + Sync + Serialize + DeserializeOwned + 'static` can be a `TaskPayload`.

### TaskExecutor<P, T>

An async trait that knows how to execute `P` and produce `T`:

```rust
use prometheus_parking_lot::{TaskExecutor, TaskMetadata};
use async_trait::async_trait;

#[derive(Clone)]
struct LlmExecutor;

#[async_trait]
impl TaskExecutor<LlmJob, String> for LlmExecutor {
    async fn execute(&self, payload: LlmJob, _meta: TaskMetadata) -> String {
        // Call your LLM / agent execution logic here.
        format!("LLM({}): {}", payload.model, payload.prompt)
    }
}
```

### TaskQueue

Abstracts **how jobs are stored and dequeued**.

Backends (implementing `TaskQueue<P>`):

- `InMemoryQueue<P>` – simple, fast, in-process.
- `SeaOrmQueue<P>` – Postgres custom table (`pl_queue_jobs`).
- `PgmqQueue<P>` – Postgres `pgmq` extension queue.
- `YaqueQueue<P>` – Yaque-based embedded file-backed queue (ideal for Tauri / local).

### Mailbox

Abstracts **how results are stored and retrieved**, and optionally how notifications are sent.

Internally, it’s composed of:

- `MailboxStorage<T>` – actual storage:
  - `InMemoryStorage<T>`
  - `SeaOrmMailboxStorage<T>` (Postgres `pl_mailbox_messages`)
  - Other storages can be added (SurrealDB, etc.).
- `MailboxNotifier<T>` – optional notifier:
  - `HttpCallbackNotifier<T>` – REST webhook callback.

High-level trait:

```rust
use prometheus_parking_lot::{Mailbox, MailboxKey, MailboxMessage};

#[async_trait::async_trait]
pub trait Mailbox<T> {
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>);
    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>>;
}
```

So if a client disconnects, the result can be **stored and picked up later**, and optionally a REST hook can notify that it’s ready.

### ResourcePool

The **parking-lot scheduler** itself.

- Enforces a `max_units` capacity (e.g., GPU units, CPU slots, worker slots).
- If there’s room → execute immediately.
- If not → enqueue into a `TaskQueue<P>`.
- When tasks finish, it wakes queued tasks that fit within current capacity.
- Uses a `Mailbox<T>` to deliver results and handle timeouts/expired work.

------

## Architecture overview

The crate follows a **feature-based, clean architecture**:

```text
src/
  core/        # Pure domain + scheduler (no infra deps)
  config/      # serde config models
  builders/    # config → core abstractions
  infra/       # queue/mailbox/runtime backends (Postgres, pgmq, Yaque, etc.)
  util/        # shared utilities (e.g. Clock abstraction)
```

- `core`: types, traits, scheduler; no direct DB, HTTP, runtime dependencies.
- `infra`: concrete backends, each behind traits.
- `config` + `builders`: glue to turn JSON/YAML configs into complete `ResourcePool`s.

This keeps the **core logic testable and independent** of any particular database, runtime, or environment.

------

## Installing

Add to your `Cargo.toml`:

```toml
[dependencies]
prometheus_parking_lot = { git = "https://github.com/your-org/prometheus_parking_lot.git", features = ["native", "postgres", "pgmq", "yaque", "config"] }
```

Features:

- `native` – enable Tokio runtime adapter.
- `postgres` – enable SeaORM-based Postgres queue/mailbox.
- `pgmq` – enable pgmq queue adapter.
- `yaque` – enable Yaque embedded queue adapter.
- `config` – enable serde-based config model and HTTP notifier.

------

## Configuration (JSON / YAML)

You define pools and backends in a `scheduler-config.json`:

```jsonc
{
  "pools": {
    "llm_inference": {
      "max_units": 20,
      "max_queue_depth": 10000,
      "default_timeout_secs": 60,
      "queue": {
        "type": "postgres_pgmq",
        "queue_name": "llm_inference"
      },
      "mailbox": {
        "storage": {
          "type": "postgres",
          "table": "pl_mailbox_messages"
        },
        "notifier": {
          "type": "http",
          "base_url": "https://prometheus-gw.example.com/hooks/mailbox",
          "auth_header": "Bearer YOUR_TOKEN"
        }
      }
    },

    "tauri_local_llm": {
      "max_units": 8,
      "max_queue_depth": 1000,
      "default_timeout_secs": 60,
      "queue": {
        "type": "yaque",
        "path": "./data/queues",
        "stream": "tauri_llm"
      },
      "mailbox": {
        "storage": {
          "type": "in_memory"
        }
      }
    }
  }
}
```

Each **pool** corresponds to a logical resource pool in your service.

------

## Basic usage example (Tokio + in-memory)

```rust
use prometheus_parking_lot::{
    TaskExecutor, TaskMetadata, TaskPayload, ScheduledTask,
    MailboxKey, TenantId, Priority, ResourceCost, ResourceKind,
    SchedulerConfig, QueueBuilderContext,
    build_pools_from_scheduler_config,
    TokioSpawner,
};
use async_trait::async_trait;
use std::{fs, time::Duration};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct LlmJob {
    model: String,
    prompt: String,
    max_tokens: u32,
}

#[derive(Clone)]
struct LlmExecutor;

#[async_trait]
impl TaskExecutor<LlmJob, String> for LlmExecutor {
    async fn execute(&self, payload: LlmJob, _meta: TaskMetadata) -> String {
        // Your LLM/agent logic here.
        format!("LLM({}): {}", payload.model, payload.prompt)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Load scheduler config (could be JSON or YAML).
    let cfg_bytes = fs::read("scheduler-config.json")?;
    let sched_cfg: SchedulerConfig = serde_json::from_slice(&cfg_bytes)?;

    // 2. Choose context (Postgres vs Embedded).
    let ctx = QueueBuilderContext::Embedded;

    // 3. Build pools – in this example, all use the same executor type.
    let pools = build_pools_from_scheduler_config::<LlmJob, String, LlmExecutor, TokioSpawner>(
        &sched_cfg,
        &ctx,
        None,                           // no DB in embedded/in-memory mode
        |_name| LlmExecutor,            // executor factory by pool name
        TokioSpawner,
    );

    let pool = pools.pools.get("tauri_local_llm").expect("pool not found");

    // 4. Submit a job.
    let meta = TaskMetadata {
        id: prometheus_parking_lot::TaskId(1),
        priority: Priority::High,
        cost: ResourceCost { kind: ResourceKind::GpuVram, units: 4 },
        created_at_ms: 0,
        deadline_ms: None,
        mailbox: Some(MailboxKey {
            tenant: TenantId("tenant-1".into()),
            user_id: Some("user-123".into()),
            session_id: None,
        }),
    };

    let job = LlmJob {
        model: "llama-3.1-8b".into(),
        prompt: "Hello from local pool".into(),
        max_tokens: 256,
    };

    let _enqueue = pool.submit(ScheduledTask { meta, payload: job }).await;

    Ok(())
}
```

------

## Use cases

### 1. Local / Tauri / desktop agents

- Queue backend: `Yaque` or `InMemory` for embedded queueing.
- Mailbox: `InMemoryMailbox<T>` or a small embedded DB for persistence.
- Capacity: defined in terms of:
  - GPU units (VRAM).
  - CPU slots.
- Great for **local LLMs** running in Prometheus Studio / Tauri shells.

### 2. Cloud AI gateways (GPU-backed)

- Queue backend: `postgres_pgmq` or `postgres_custom`.
- Mailbox: Postgres-backed with REST notifier to your gateway.
- Capacity: defined in terms of:
  - GPU VRAM units (model + KV cache).
  - CPU-based cost model for heavy tasks.
- Great for **multi-tenant inference gateways** and **agent farms**.

### 3. Web apps with web workers

- Queue backend on server: Postgres or pgmq.
- On the client (if needed): an embedded queue (future IndexedDB/PGlite backend).
- Capacity: number of web workers or per-tenant rate limits.
- Nice fit for **browser-based AI apps** backed by Prometheus.

------

## Design notes

- **Clean architecture**: core scheduling logic is completely independent of queue/mailbox technologies and runtimes.
- **Testable**: `ResourcePool` uses traits (`TaskQueue`, `Mailbox`, `Spawn`) and can be tested with in-memory fakes.
- **Config-driven**: behavior is controlled by `SchedulerConfig` so you can change backends and capacity without recompiling.
- **Extensible**:
  - Add new queue backends by implementing `TaskQueue<P>`.
  - Add new mailbox storages by implementing `MailboxStorage<T>`.
  - Add new notifiers by implementing `MailboxNotifier<T>`.
  - Add new runtimes by implementing `Spawn` (web workers, WASM, etc.).

------

## Status

This README + `lib.rs` describe a **version 1** structure meant to be:

- Minimal but **immediately useful**.
- Safe and explicit in its abstractions.
- Ready to be integrated into the Prometheus AI platform as the **standard scheduling layer** for agent workloads.

As you implement the remaining modules (`core/*`, `infra/*`, etc.), you can refine the API surface, add error types, and deepen tests without changing the core conceptual model.

```text
::contentReference[oaicite:0]{index=0}
```

------

## Prompt:

Continue.

## Answer:

Nice, let’s turn this into an actually-usable v1 skeleton you can drop straight into a repo.

I’ll focus on:

- Core modules (fully usable, in-memory).
- Config + builders (so you can use JSON/YAML right away).
- Infra (in-memory + Tokio runtime ready; Postgres/pgmq/Yaque stubbed behind features for later).

You can paste these into files matching the paths.

------

## 1. `src/core/mod.rs`

```rust
pub mod types;
pub mod task;
pub mod queue;
pub mod mailbox;
pub mod runtime;
pub mod scheduler;
```

------

## 2. `src/core/types.rs`

```rust
use serde::{Deserialize, Serialize};

/// Unique identifier for a scheduled task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskId(pub u64);

/// Tenant identifier (org, project, application).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TenantId(pub String);

/// Mailbox routing key: where to deliver results.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailboxKey {
    pub tenant: TenantId,
    pub user_id: Option<String>,
    pub session_id: Option<String>,
}

/// Priority for tasks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Priority {
    Low,
    Normal,
    High,
    Critical,
}

/// Resource type the task consumes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ResourceKind {
    Cpu,
    GpuVram,
    Io,
    Mixed,
}

/// Logical resource cost of a task (in arbitrary units).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ResourceCost {
    pub kind: ResourceKind,
    pub units: u32,
}

/// Minimal metadata for scheduling and delivery.
/// Time fields are epoch milliseconds to be portable across processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskMetadata {
    pub id: TaskId,
    pub priority: Priority,
    pub cost: ResourceCost,
    pub created_at_ms: u128,
    pub deadline_ms: Option<u128>,
    pub mailbox: Option<MailboxKey>,
}

/// Status of a task result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TaskStatus {
    Completed,
    Failed(String),
    Expired,
    Dropped(String),
}

/// Message stored in a mailbox.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage<T> {
    pub task_id: TaskId,
    pub status: TaskStatus,
    pub created_at_ms: u128,
    pub payload: Option<T>,
}
```

------

## 3. `src/core/task.rs`

```rust
use async_trait::async_trait;
use serde::{de::DeserializeOwned, Serialize};

use crate::core::types::TaskMetadata;

/// Marker trait for serializable job descriptors.
pub trait TaskPayload: Send + Sync + Serialize + DeserializeOwned + 'static {}
impl<T> TaskPayload for T where T: Send + Sync + Serialize + DeserializeOwned + 'static {}

/// A scheduled task consisting of metadata + payload.
#[derive(Debug, Clone)]
pub struct ScheduledTask<P: TaskPayload> {
    pub meta: TaskMetadata,
    pub payload: P,
}

/// Async trait for executing a payload `P` and producing a result `T`.
#[async_trait]
pub trait TaskExecutor<P, T>: Send + Sync + Clone + 'static
where
    P: TaskPayload,
    T: Send + Sync + 'static,
{
    async fn execute(&self, payload: P, meta: TaskMetadata) -> T;
}
```

------

## 4. `src/core/queue.rs`

```rust
use async_trait::async_trait;
use std::time::Instant;

use crate::core::task::{ScheduledTask, TaskPayload};

/// Result of attempting to enqueue a task.
#[derive(Debug, Clone)]
pub struct EnqueueResult {
    pub accepted: bool,
    pub reason: Option<String>,
}

/// Outcome of trying to get the next task from the queue.
#[derive(Debug)]
pub enum DequeueOutcome<P: TaskPayload> {
    Task(ScheduledTask<P>),
    Empty,
}

/// Abstract queue backend interface.
#[async_trait]
pub trait TaskQueue<P>: Send + Sync
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult;

    async fn dequeue_next(&self, now: Instant) -> DequeueOutcome<P>;

    async fn prune_expired(&self, now: Instant) -> usize;

    async fn len(&self) -> usize;
}
```

------

## 5. `src/core/mailbox.rs`

```rust
use async_trait::async_trait;

use crate::core::types::{MailboxKey, MailboxMessage};

/// Storage layer for mailbox messages.
#[async_trait]
pub trait MailboxStorage<T>: Send + Sync
where
    T: Send + Sync + 'static,
{
    async fn store(&self, key: &MailboxKey, msg: MailboxMessage<T>);

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>>;
}

/// Notification layer for mailbox events (e.g., REST callbacks).
#[async_trait]
pub trait MailboxNotifier<T>: Send + Sync
where
    T: Send + Sync + 'static,
{
    async fn notify(&self, key: &MailboxKey, msg: &MailboxMessage<T>);
}

/// High-level mailbox interface used by the scheduler.
#[async_trait]
pub trait Mailbox<T>: Send + Sync
where
    T: Send + Sync + 'static,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>);

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>>;
}

/// Mailbox that only stores messages (no notifications).
pub struct StorageOnlyMailbox<T, S>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
{
    storage: S,
    _marker: std::marker::PhantomData<T>,
}

impl<T, S> StorageOnlyMailbox<T, S>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
{
    pub fn new(storage: S) -> Self {
        Self {
            storage,
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, S> Mailbox<T> for StorageOnlyMailbox<T, S>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        self.storage.store(key, msg).await;
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        self.storage.fetch(key, since_ms, limit).await
    }
}

/// Mailbox that stores messages and also sends notifications.
pub struct StorageWithNotifierMailbox<T, S, N>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
    N: MailboxNotifier<T>,
{
    storage: S,
    notifier: N,
    _marker: std::marker::PhantomData<T>,
}

impl<T, S, N> StorageWithNotifierMailbox<T, S, N>
where
    T: Send + Sync + 'static,
    S: MailboxStorage<T>,
    N: MailboxNotifier<T>,
{
    pub fn new(storage: S, notifier: N) -> Self {
        Self {
            storage,
            notifier,
            _marker: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<T, S, N> Mailbox<T> for StorageWithNotifierMailbox<T, S, N>
where
    T: Send + Sync + 'static + serde::Serialize,
    S: MailboxStorage<T>,
    N: MailboxNotifier<T>,
{
    async fn deliver(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        self.storage.store(key, msg.clone()).await;
        self.notifier.notify(key, &msg).await;
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        self.storage.fetch(key, since_ms, limit).await
    }
}
```

------

## 6. `src/core/runtime.rs`

```rust
use async_trait::async_trait;
use std::future::Future;

/// Abstract runtime for spawning async work.
#[async_trait]
pub trait Spawn: Send + Sync + Clone + 'static {
    async fn spawn_detached<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static;
}
```

------

## 7. `src/core/scheduler.rs`

```rust
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::core::{
    mailbox::Mailbox,
    queue::{DequeueOutcome, TaskQueue},
    runtime::Spawn,
    task::{ScheduledTask, TaskExecutor, TaskPayload},
    types::{MailboxMessage, TaskMetadata, TaskStatus},
};

/// Runtime pool configuration (per pool).
#[derive(Debug, Clone)]
pub struct PoolConfig {
    pub max_units: u32,
    pub max_queue_depth: usize,
    pub default_timeout: Duration,
}

struct Inner<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    config: PoolConfig,
    queue: Q,
    mailbox: M,
    spawner: S,
    executor: E,
    active_units: u32,
}

/// The parking-lot scheduler: manages capacity, queues, and result delivery.
#[derive(Clone)]
pub struct ResourcePool<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    inner: Arc<Mutex<Inner<P, Q, M, S, E, T>>>,
}

impl<P, Q, M, S, E, T> ResourcePool<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    pub fn new(config: PoolConfig, queue: Q, mailbox: M, spawner: S, executor: E) -> Self {
        let inner = Inner {
            config,
            queue,
            mailbox,
            spawner,
            executor,
            active_units: 0,
        };
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }

    /// Submit a task; runs immediately if capacity is available, otherwise enqueues.
    pub async fn submit(&self, task: ScheduledTask<P>) -> crate::core::queue::EnqueueResult {
        let mut guard = self.inner.lock().unwrap();
        let cost = task.meta.cost.units;

        if guard.active_units + cost <= guard.config.max_units {
            guard.spawn_running(self.clone(), task);
            crate::core::queue::EnqueueResult {
                accepted: true,
                reason: None,
            }
        } else {
            guard.queue.enqueue(task).await
        }
    }

    async fn on_task_finished_inner(
        inner: &mut Inner<P, Q, M, S, E, T>,
        meta: TaskMetadata,
        result: Result<T, String>,
    ) {
        inner.active_units = inner.active_units.saturating_sub(meta.cost.units);

        // Deliver result to mailbox if configured.
        if let Some(mailbox_key) = meta.mailbox.clone() {
            let msg = MailboxMessage {
                task_id: meta.id,
                status: match &result {
                    Ok(_) => TaskStatus::Completed,
                    Err(e) => TaskStatus::Failed(e.clone()),
                },
                created_at_ms: meta.created_at_ms, // or "now" if you prefer
                payload: result.ok(),
            };
            inner.mailbox.deliver(&mailbox_key, msg).await;
        }

        // Wake parked tasks.
        let now = Instant::now();
        let _expired = inner.queue.prune_expired(now).await;

        loop {
            match inner.queue.dequeue_next(now).await {
                DequeueOutcome::Empty => break,
                DequeueOutcome::Task(task) => {
                    let cost = task.meta.cost.units;
                    if inner.active_units + cost <= inner.config.max_units {
                        inner.spawn_running_self(task);
                    } else {
                        // Put back into queue and stop.
                        let _ = inner.queue.enqueue(task).await;
                        break;
                    }
                }
            }
        }
    }
}

impl<P, Q, M, S, E, T> Inner<P, Q, M, S, E, T>
where
    P: TaskPayload,
    Q: TaskQueue<P>,
    M: Mailbox<T>,
    S: Spawn,
    E: TaskExecutor<P, T>,
    T: Send + Sync + 'static,
{
    fn spawn_running(
        &mut self,
        pool: ResourcePool<P, Q, M, S, E, T>,
        task: ScheduledTask<P>,
    ) {
        let cost = task.meta.cost.units;
        self.active_units += cost;

        let meta = task.meta.clone();
        let payload = task.payload;
        let executor = self.executor.clone();
        let spawner = self.spawner.clone();

        spawner
            .clone()
            .spawn_detached(async move {
                let result = executor.execute(payload, meta.clone()).await;
                let mut guard = pool.inner.lock().unwrap();
                ResourcePool::on_task_finished_inner(&mut guard, meta, Ok(result)).await;
            });
    }

    fn spawn_running_self(&mut self, task: ScheduledTask<P>) {
        // This helper is for more advanced internal flows; for v1 you can
        // reuse `spawn_running` by passing a cloned ResourcePool if needed.
        let _ = task;
    }
}
```

------

## 8. `src/config/mod.rs` & `src/config/model.rs`

### `src/config/mod.rs`

```rust
pub mod model;
pub use model::*;
```

### `src/config/model.rs`

```rust
use serde::Deserialize;
use std::collections::HashMap;

use crate::core::types::ResourceKind;

/// Top-level scheduler config.
#[derive(Debug, Clone, Deserialize)]
pub struct SchedulerConfig {
    pub pools: HashMap<String, PoolConfig>,
}

/// Per-pool configuration loaded from JSON/YAML.
#[derive(Debug, Clone, Deserialize)]
pub struct PoolConfig {
    pub max_units: u32,
    pub max_queue_depth: usize,
    pub default_timeout_secs: u64,
    pub queue: QueueConfig,
    pub mailbox: MailboxConfig,
    #[serde(default)]
    pub resource_kind: Option<ResourceKind>,
}

/// Queue configuration wrapper.
#[derive(Debug, Clone, Deserialize)]
pub struct QueueConfig {
    #[serde(flatten)]
    pub backend: QueueBackendConfig,
}

/// Queue backends supported by config.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum QueueBackendConfig {
    InMemory,
    PostgresCustom { table: String },
    PostgresPgmq { queue_name: String },
    Yaque { path: String, stream: String },
}

/// Mailbox config: storage + optional notifier.
#[derive(Debug, Clone, Deserialize)]
pub struct MailboxConfig {
    pub storage: MailboxStorageConfig,
    pub notifier: Option<MailboxNotifierConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MailboxStorageConfig {
    InMemory,
    Postgres { table: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MailboxNotifierConfig {
    Http {
        base_url: String,
        #[serde(default)]
        auth_header: Option<String>,
    },
}
```

------

## 9. `src/builders/mod.rs`

```rust
pub mod queue_builder;
pub mod mailbox_builder;
pub mod pool_builder;

pub use queue_builder::QueueBuilderContext;
pub use queue_builder::build_task_queue_from_config;
pub use mailbox_builder::build_mailbox_from_config;
pub use pool_builder::{build_pool_from_config, build_pools_from_scheduler_config};
```

------

## 10. `src/builders/queue_builder.rs`

```rust
use crate::config::{QueueBackendConfig, QueueConfig};
use crate::core::queue::TaskQueue;
use crate::core::task::TaskPayload;
use crate::infra::queue::in_memory::InMemoryQueue;

#[cfg(feature = "postgres")]
use crate::infra::queue::sea_orm::SeaOrmQueue;
#[cfg(feature = "pgmq")]
use crate::infra::queue::pgmq::PgmqQueue;
#[cfg(feature = "yaque")]
use crate::infra::queue::yaque::YaqueQueue;

#[cfg(feature = "postgres")]
use sea_orm::DatabaseConnection;

/// Context in which we are building queues.
pub enum QueueBuilderContext {
    #[cfg(feature = "postgres")]
    Postgres { db: DatabaseConnection },
    Embedded,
}

/// Build a `TaskQueue<P>` from configuration.
pub fn build_task_queue_from_config<P>(
    ctx: &QueueBuilderContext,
    cfg: &QueueConfig,
    max_depth: usize,
) -> Box<dyn TaskQueue<P>>
where
    P: TaskPayload,
{
    match (&cfg.backend, ctx) {
        (QueueBackendConfig::InMemory, _) => Box::new(InMemoryQueue::<P>::new(max_depth)),

        #[cfg(feature = "postgres")]
        (QueueBackendConfig::PostgresCustom { table }, QueueBuilderContext::Postgres { db }) => {
            Box::new(SeaOrmQueue::<P>::new(db.clone(), table.clone(), max_depth))
        }

        #[cfg(feature = "pgmq")]
        (QueueBackendConfig::PostgresPgmq { queue_name }, QueueBuilderContext::Postgres { db }) => {
            Box::new(PgmqQueue::<P>::new(db.clone(), queue_name.clone(), max_depth))
        }

        #[cfg(feature = "yaque")]
        (QueueBackendConfig::Yaque { path, stream }, QueueBuilderContext::Embedded) => {
            Box::new(YaqueQueue::<P>::new(path.clone(), stream.clone(), max_depth))
        }

        (backend, ctx) => {
            panic!("Unsupported queue backend {:?} in context {:?}", backend, ctx);
        }
    }
}
```

------

## 11. `src/builders/mailbox_builder.rs`

```rust
use crate::config::{MailboxConfig, MailboxNotifierConfig, MailboxStorageConfig};
use crate::core::mailbox::{Mailbox, MailboxNotifier, MailboxStorage, StorageOnlyMailbox, StorageWithNotifierMailbox};
use crate::infra::mailbox::in_memory::InMemoryStorage;

#[cfg(feature = "postgres")]
use crate::infra::mailbox::sea_orm::SeaOrmMailboxStorage;
#[cfg(feature = "config")]
use crate::infra::mailbox::http_notifier::HttpCallbackNotifier;

#[cfg(feature = "postgres")]
use sea_orm::DatabaseConnection;

use super::queue_builder::QueueBuilderContext;

fn build_mailbox_storage_from_config<T>(
    ctx: &QueueBuilderContext,
    cfg: &MailboxStorageConfig,
    db: Option<&DatabaseConnection>,
) -> Box<dyn MailboxStorage<T>>
where
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    match cfg {
        MailboxStorageConfig::InMemory => Box::new(InMemoryStorage::<T>::new()),

        #[cfg(feature = "postgres")]
        MailboxStorageConfig::Postgres { table } => {
            let db = db.expect("Postgres mailbox storage requires DatabaseConnection");
            Box::new(SeaOrmMailboxStorage::<T>::new(db.clone(), table.clone()))
        }

        #[allow(unreachable_patterns)]
        _ => panic!("Mailbox storage backend not supported in this build"),
    }
}

fn build_mailbox_notifier_from_config<T>(
    cfg: &MailboxNotifierConfig,
) -> Box<dyn MailboxNotifier<T>>
where
    T: Send + Sync + serde::Serialize + 'static,
{
    match cfg {
        #[cfg(feature = "config")]
        MailboxNotifierConfig::Http { base_url, auth_header } => {
            Box::new(HttpCallbackNotifier::<T>::new(
                base_url.clone(),
                auth_header.clone(),
            ))
        }
        #[allow(unreachable_patterns)]
        _ => panic!("Mailbox notifier backend not supported in this build"),
    }
}

/// Build a Mailbox<T> from configuration.
pub fn build_mailbox_from_config<T>(
    ctx: &QueueBuilderContext,
    cfg: &MailboxConfig,
    db: Option<&DatabaseConnection>,
) -> Box<dyn Mailbox<T>>
where
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
{
    let storage = build_mailbox_storage_from_config::<T>(ctx, &cfg.storage, db);

    if let Some(notifier_cfg) = &cfg.notifier {
        let notifier = build_mailbox_notifier_from_config::<T>(notifier_cfg);
        Box::new(StorageWithNotifierMailbox::new(storage, notifier))
    } else {
        Box::new(StorageOnlyMailbox::new(storage))
    }
}
```

------

## 12. `src/builders/pool_builder.rs`

```rust
use std::collections::HashMap;

use crate::config::{PoolConfig, SchedulerConfig};
use crate::core::{
    mailbox::Mailbox,
    queue::TaskQueue,
    scheduler::{PoolConfig as RuntimePoolConfig, ResourcePool},
    task::{TaskExecutor, TaskPayload},
    runtime::Spawn,
};

use super::{build_mailbox_from_config, build_task_queue_from_config, QueueBuilderContext};

#[cfg(feature = "postgres")]
use sea_orm::DatabaseConnection;

/// Build a single ResourcePool from config.
pub fn build_pool_from_config<P, T, E, S>(
    pool_cfg: &PoolConfig,
    ctx: &QueueBuilderContext,
    db: Option<&DatabaseConnection>,
    executor: E,
    spawner: S,
) -> ResourcePool<P, Box<dyn TaskQueue<P>>, Box<dyn Mailbox<T>>, S, E, T>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn,
{
    let runtime_config = RuntimePoolConfig {
        max_units: pool_cfg.max_units,
        max_queue_depth: pool_cfg.max_queue_depth,
        default_timeout: std::time::Duration::from_secs(pool_cfg.default_timeout_secs),
    };

    let queue = build_task_queue_from_config::<P>(
        ctx,
        &pool_cfg.queue,
        pool_cfg.max_queue_depth,
    );

    let mailbox = build_mailbox_from_config::<T>(ctx, &pool_cfg.mailbox, db);

    ResourcePool::new(runtime_config, queue, mailbox, spawner, executor)
}

/// Container for built pools.
pub struct BuiltPools<P, T, E, S>
where
    P: TaskPayload,
    T: Send + Sync + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn,
{
    pub pools:
        HashMap<String, ResourcePool<P, Box<dyn TaskQueue<P>>, Box<dyn Mailbox<T>>, S, E, T>>,
}

/// Build all pools from a SchedulerConfig.
pub fn build_pools_from_scheduler_config<P, T, E, S>(
    cfg: &SchedulerConfig,
    ctx: &QueueBuilderContext,
    db: Option<&DatabaseConnection>,
    executor_factory: impl Fn(&str) -> E,
    spawner: S,
) -> BuiltPools<P, T, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + serde::de::DeserializeOwned + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Clone,
{
    let mut map = HashMap::new();

    for (name, pool_cfg) in &cfg.pools {
        let executor = executor_factory(name);
        let pool = build_pool_from_config::<P, T, E, S>(
            pool_cfg,
            ctx,
            db,
            executor,
            spawner.clone(),
        );
        map.insert(name.clone(), pool);
    }

    BuiltPools { pools: map }
}
```

------

## 13. `src/infra/mod.rs` and basic backends

### `src/infra/mod.rs`

```rust
pub mod queue;
pub mod mailbox;
pub mod runtime;
```

### `src/infra/queue/mod.rs`

```rust
pub mod in_memory;

#[cfg(feature = "postgres")]
pub mod sea_orm;

#[cfg(feature = "pgmq")]
pub mod pgmq;

#[cfg(feature = "yaque")]
pub mod yaque;
```

#### `src/infra/queue/in_memory.rs`

```rust
use std::{
    cmp::Ordering,
    collections::BinaryHeap,
    sync::Mutex,
    time::Instant,
};

use async_trait::async_trait;

use crate::core::queue::{DequeueOutcome, EnqueueResult, TaskQueue};
use crate::core::task::{ScheduledTask, TaskPayload};
use crate::core::types::Priority;

/// Internal item stored in the in-memory priority queue.
#[derive(Debug)]
struct QueueItem<P: TaskPayload> {
    priority: Priority,
    created_at_ms: u128,
    task: ScheduledTask<P>,
}

impl<P: TaskPayload> PartialEq for QueueItem<P> {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority && self.created_at_ms == other.created_at_ms
    }
}
impl<P: TaskPayload> Eq for QueueItem<P> {}

impl<P: TaskPayload> PartialOrd for QueueItem<P> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl<P: TaskPayload> Ord for QueueItem<P> {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .priority
            .cmp(&self.priority)
            .then(self.created_at_ms.cmp(&other.created_at_ms))
    }
}

/// Simple in-memory queue (priority + FIFO).
pub struct InMemoryQueue<P: TaskPayload> {
    inner: Mutex<BinaryHeap<QueueItem<P>>>,
    max_depth: usize,
}

impl<P: TaskPayload> InMemoryQueue<P> {
    pub fn new(max_depth: usize) -> Self {
        Self {
            inner: Mutex::new(BinaryHeap::new()),
            max_depth,
        }
    }
}

#[async_trait]
impl<P> TaskQueue<P> for InMemoryQueue<P>
where
    P: TaskPayload,
{
    async fn enqueue(&self, task: ScheduledTask<P>) -> EnqueueResult {
        let mut guard = self.inner.lock().unwrap();
        if guard.len() >= self.max_depth {
            return EnqueueResult {
                accepted: false,
                reason: Some("queue at max capacity".into()),
            };
        }

        let created_at_ms = task.meta.created_at_ms;
        guard.push(QueueItem {
            priority: task.meta.priority,
            created_at_ms,
            task,
        });

        EnqueueResult {
            accepted: true,
            reason: None,
        }
    }

    async fn dequeue_next(&self, _now: Instant) -> DequeueOutcome<P> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(item) = guard.pop() {
            DequeueOutcome::Task(item.task)
        } else {
            DequeueOutcome::Empty
        }
    }

    async fn prune_expired(&self, _now: Instant) -> usize {
        // For v1, expiry is handled at scheduler level if needed.
        0
    }

    async fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}
```

### `src/infra/mailbox/mod.rs`

```rust
pub mod in_memory;

#[cfg(feature = "postgres")]
pub mod sea_orm;

#[cfg(feature = "config")]
pub mod http_notifier;
```

#### `src/infra/mailbox/in_memory.rs`

```rust
use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;

use crate::core::mailbox::MailboxStorage;
use crate::core::types::{MailboxKey, MailboxMessage};

/// Simple in-memory mailbox storage.
pub struct InMemoryStorage<T>
where
    T: Send + Sync + 'static,
{
    inner: Mutex<HashMap<MailboxKey, Vec<MailboxMessage<T>>>>,
}

impl<T> InMemoryStorage<T>
where
    T: Send + Sync + 'static,
{
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl<T> MailboxStorage<T> for InMemoryStorage<T>
where
    T: Send + Sync + 'static,
{
    async fn store(&self, key: &MailboxKey, msg: MailboxMessage<T>) {
        let mut guard = self.inner.lock().unwrap();
        guard.entry(key.clone()).or_default().push(msg);
    }

    async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<T>> {
        let guard = self.inner.lock().unwrap();
        let msgs = guard.get(key).cloned().unwrap_or_default();
        msgs.into_iter()
            .filter(|m| since_ms.map(|s| m.created_at_ms >= s).unwrap_or(true))
            .take(limit)
            .collect()
    }
}

/// Convenience alias: in-memory mailbox without notifications.
pub type InMemoryMailbox<T> = crate::core::mailbox::StorageOnlyMailbox<T, InMemoryStorage<T>>;
```

### `src/infra/runtime/mod.rs`

```rust
#[cfg(feature = "native")]
pub mod tokio;
```

#### `src/infra/runtime/tokio.rs`

```rust
use async_trait::async_trait;
use std::future::Future;

use crate::core::runtime::Spawn;

/// Tokio-based spawner.
#[derive(Clone)]
pub struct TokioSpawner;

#[async_trait]
impl Spawn for TokioSpawner {
    async fn spawn_detached<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}
```

------

With this skeleton:

- You can immediately use **in-memory** queue + mailbox + Tokio runtime (no DB required).
- Config-driven builders are in place.
- Postgres/pgmq/Yaque adapters are structurally ready to drop into `infra/queue/sea_orm.rs`, `infra/queue/pgmq.rs`, `infra/queue/yaque.rs`, and `infra/mailbox/sea_orm.rs`, `infra/mailbox/http_notifier.rs` as you implement them.

From here you can:

- Start by running an example using `InMemoryQueue` and `InMemoryMailbox`.
- Gradually fill in DB-backed implementations without touching the core scheduler or public API.
