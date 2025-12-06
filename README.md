# prometheus_parking_lot

A configurable, runtime-agnostic **parking-lot scheduler** for Prometheus AI agents and services.

This crate provides a dedicated scheduling layer that:

- Manages **resource-constrained pools** (CPU, GPU VRAM, web workers, etc.).
- Queues excess work in a **"parking lot"** and wakes it when capacity is free.
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

- Limit concurrency by **real resource usage**, not just "number of tasks".
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
```

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

### TaskQueue<P>

Abstracts how jobs are stored and dequeued.

**Backends** (implementing `TaskQueue<P>`):

- `InMemoryQueue<P>` – simple, fast, in-process.
- `SeaOrmQueue<P>` – Postgres custom table (pl_queue_jobs).
- `PgmqQueue<P>` – Postgres pgmq extension queue.
- `YaqueQueue<P>` – Yaque-based embedded file-backed queue (ideal for Tauri / local).

### Mailbox<T>

Abstracts how results are stored and retrieved, and optionally how notifications are sent.

Internally, it's composed of:

- **MailboxStorage<T>** – actual storage:
  - `InMemoryStorage<T>`
  - `SeaOrmMailboxStorage<T>` (Postgres pl_mailbox_messages)
  - Other storages can be added (SurrealDB, etc.).

- **MailboxNotifier<T>** – optional notifier:
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

So if a client disconnects, the result can be stored and picked up later, and optionally a REST hook can notify that it's ready.

### ResourcePool

The parking-lot scheduler itself.

- Enforces a `max_units` capacity (e.g., GPU units, CPU slots, worker slots).
- If there's room → execute immediately.
- If not → enqueue into a `TaskQueue<P>`.
- When tasks finish, it wakes queued tasks that fit within current capacity.
- Uses a `Mailbox<T>` to deliver results and handle timeouts/expired work.

---

## Architecture overview

The crate follows a feature-based, clean architecture:

```
src/
  core/        # Pure domain + scheduler (no infra deps)
  config/      # serde config models
  builders/    # config → core abstractions
  infra/       # queue/mailbox/runtime backends (Postgres, pgmq, Yaque, etc.)
  util/        # shared utilities (e.g. Clock abstraction)
```

- **core**: types, traits, scheduler; no direct DB, HTTP, runtime dependencies.
- **infra**: concrete backends, each behind traits.
- **config + builders**: glue to turn JSON/YAML configs into complete ResourcePools.

This keeps the core logic testable and independent of any particular database, runtime, or environment.

---

## Installing

Add to your `Cargo.toml`:

```toml
[dependencies]
prometheus_parking_lot = { git = "https://github.com/your-org/prometheus_parking_lot.git", features = ["native", "postgres", "pgmq", "yaque", "config"] }
```

**Features:**

- `native` – enable Tokio runtime adapter.
- `postgres` – enable SeaORM-based Postgres queue/mailbox.
- `pgmq` – enable pgmq queue adapter.
- `yaque` – enable Yaque embedded queue adapter.
- `config` – enable serde-based config model and HTTP notifier.

---

## Configuration (JSON / YAML)

You define pools and backends in a `scheduler-config.json`:

```json
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

Each pool corresponds to a logical resource pool in your service.

---

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

---

## Use cases

### 1. Local / Tauri / desktop agents

- **Queue backend:** Yaque or InMemory for embedded queueing.
- **Mailbox:** `InMemoryMailbox<T>` or a small embedded DB for persistence.
- **Capacity:** defined in terms of:
  - GPU units (VRAM).
  - CPU slots.
- Great for local LLMs running in Prometheus Studio / Tauri shells.

### 2. Cloud AI gateways (GPU-backed)

- **Queue backend:** `postgres_pgmq` or `postgres_custom`.
- **Mailbox:** Postgres-backed with REST notifier to your gateway.
- **Capacity:** defined in terms of:
  - GPU VRAM units (model + KV cache).
  - CPU-based cost model for heavy tasks.
- Great for multi-tenant inference gateways and agent farms.

### 3. Web apps with web workers

- **Queue backend on server:** Postgres or pgmq.
- **On the client (if needed):** an embedded queue (future IndexedDB/PGlite backend).
- **Capacity:** number of web workers or per-tenant rate limits.
- Nice fit for browser-based AI apps backed by Prometheus.

---

## Design notes

- **Clean architecture:** core scheduling logic is completely independent of queue/mailbox technologies and runtimes.
- **Testable:** `ResourcePool` uses traits (`TaskQueue`, `Mailbox`, `Spawn`) and can be tested with in-memory fakes.
- **Config-driven:** behavior is controlled by `SchedulerConfig` so you can change backends and capacity without recompiling.
- **Extensible:**
  - Add new queue backends by implementing `TaskQueue<P>`.
  - Add new mailbox storages by implementing `MailboxStorage<T>`.
  - Add new notifiers by implementing `MailboxNotifier<T>`.
  - Add new runtimes by implementing `Spawn` (web workers, WASM, etc.).

---

## Status

🚧 **Version 1.0 in Development** - This library is following a phased implementation plan with clean architecture principles.

See the [docs/](docs/) directory for detailed specifications and implementation plans.

---

## Documentation

For comprehensive documentation, see:

- [DESIGN.md](docs/DESIGN.md) - Architecture and design decisions
- [IMPLEMENTATION.md](docs/IMPLEMENTATION.md) - Implementation roadmap
- [API.md](docs/API.md) - Detailed API documentation
- [CHANGELOG.md](docs/CHANGELOG.md) - Version history

---

## License

Licensed under either of:

- MIT license ([LICENSE-MIT](LICENSE-MIT))
- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))

at your option.

---

*This README describes a version 1 structure meant to be minimal but immediately useful, safe and explicit in its abstractions, and ready to be integrated into the Prometheus AI platform as the standard scheduling layer for agent workloads.*
