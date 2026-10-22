# prometheus_parking_lot

A configurable, runtime-agnostic **parking-lot scheduler** for Prometheus AI agents and services.

## Overview

This crate provides a dedicated scheduling layer that manages resource-constrained workloads across different deployment environments. It implements a "parking lot" pattern where tasks are intelligently queued when system capacity is exhausted, then automatically woken when resources become available.

## Core Problem Solved

AI workloads have fundamentally different resource constraints than typical web services:

- **GPU VRAM Limits**: Running multiple LLM inference tasks can exceed available GPU memory, causing OOM crashes
- **Expensive Task Loss**: AI tasks are computationally expensive - losing work due to restarts or disconnections is costly
- **Multi-Environment Deployment**: The same scheduling logic needs to work in desktop apps (Tauri), cloud services, and web applications
- **Disconnected Clients**: Long-running AI tasks may complete after clients disconnect, requiring result storage and retrieval

## Architecture

The crate follows a **feature-based clean architecture**:

```
src/
  core/        # Pure domain + scheduler (no infra deps)
  config/      # serde config models
  builders/    # config → core abstractions
  infra/       # queue/mailbox/runtime backends
  util/        # shared utilities (Clock abstraction)
```

### Core Abstractions

- **TaskPayload**: Serializable job descriptors (e.g., LLM inference requests)
- **TaskExecutor<P, T>**: Async trait that executes payloads and returns results
- **TaskQueue<P>**: Abstract queue backend (in-memory, Postgres, pgmq, Yaque)
- **Mailbox<T>**: Result delivery and retrieval with optional notifications
- **ResourcePool**: The core parking-lot scheduler enforcing capacity limits

## Supported Backends

### Queue Backends

- **InMemory**: Fast, simple, no persistence (development/testing)
- **Postgres Custom Table**: SeaORM-based with `pl_queue_jobs` table
- **Postgres pgmq**: Uses pgmq extension for high-throughput queuing
- **Yaque**: Embedded file-backed queue (ideal for Tauri/desktop apps)

### Mailbox Backends

- **Storage Layer**:
  - `InMemoryStorage<T>`: Fast, no persistence
  - `SeaOrmMailboxStorage<T>`: Postgres-backed using `pl_mailbox_messages`
- **Notification Layer** (optional):
  - `HttpCallbackNotifier<T>`: REST webhook callbacks

## Configuration

Behavior is controlled via JSON/YAML configuration:

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

## Usage Example

```rust
use prometheus_parking_lot::*;
use async_trait::async_trait;

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
        // Your LLM/agent logic here
        format!("LLM({}): {}", payload.model, payload.prompt)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration
    let cfg_bytes = std::fs::read("scheduler-config.json")?;
    let sched_cfg: SchedulerConfig = serde_json::from_slice(&cfg_bytes)?;

    // Build pools from config
    let ctx = QueueBuilderContext::Embedded;
    let pools = build_pools_from_scheduler_config::<LlmJob, String, LlmExecutor, TokioSpawner>(
        &sched_cfg,
        &ctx,
        None,
        |_name| LlmExecutor,
        TokioSpawner,
    );

    let pool = pools.pools.get("tauri_local_llm").expect("pool not found");

    // Submit a job
    let meta = TaskMetadata {
        id: TaskId(1),
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

    let _result = pool.submit(ScheduledTask { meta, payload: job }).await;

    Ok(())
}
```

## Database Schema

### Queue Table (pl_queue_jobs)

```sql
CREATE TABLE IF NOT EXISTS pl_queue_jobs (
    id              BIGSERIAL PRIMARY KEY,
    queue_name      TEXT NOT NULL,
    priority        SMALLINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms   BIGINT      NOT NULL,
    deadline_at     TIMESTAMPTZ NULL,
    deadline_ms     BIGINT      NULL,
    metadata        JSONB NOT NULL,
    payload         JSONB NOT NULL,
    locked_by       TEXT        NULL,
    locked_at       TIMESTAMPTZ NULL,
    attempts        INTEGER     NOT NULL DEFAULT 0
);

CREATE INDEX idx_pl_queue_jobs_dequeue
    ON pl_queue_jobs (queue_name, priority DESC, created_at ASC);
```

### Mailbox Table (pl_mailbox_messages)

```sql
CREATE TABLE IF NOT EXISTS pl_mailbox_messages (
    id              BIGSERIAL PRIMARY KEY,
    tenant          TEXT    NOT NULL,
    user_id         TEXT    NULL,
    session_id      TEXT    NULL,
    task_id         BIGINT  NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms   BIGINT      NOT NULL,
    status          TEXT        NOT NULL,
    payload         JSONB       NULL
);

CREATE INDEX idx_pl_mailbox_messages_lookup
    ON pl_mailbox_messages (tenant, user_id, session_id, created_at_ms);
```

## Key Features

### Resource-Aware Scheduling
- Tracks resource consumption in arbitrary units (GPU VRAM, CPU slots, etc.)
- Enforces capacity limits to prevent resource exhaustion
- Supports different resource kinds (CPU, GpuVram, IO, Mixed)

### Parking Lot Algorithm
1. Task arrives → check if `active_units + task_cost <= max_units`
2. If yes → spawn immediately
3. If no → enqueue in priority queue (parking lot)
4. When running task finishes → wake next queued task that fits

### Persistent Queues
- Survive application restarts
- Multiple backend options for different environments
- Priority-based dequeuing with FIFO within priority levels

### Mailbox System
- Store results for later retrieval when clients disconnect
- Composable: storage + optional notifications
- Support for REST callbacks to notify external systems

### Multi-Environment Support
- **Desktop/Tauri**: Yaque file-backed queues, local storage
- **Cloud**: Postgres/pgmq queues, database storage, REST notifications
- **Web**: Flexible backend selection based on deployment needs

## Implementation Status

Based on the previous conversation, the following components have been designed:

### ✅ Complete
- Core domain model and traits
- Configuration schema and types
- Builder pattern for config-driven setup
- Database migrations for Postgres backends
- SeaORM entity models
- Architecture and module structure

### 🚧 In Progress
- Infrastructure module implementations
- Utility modules (Clock abstraction)
- Error handling with crate-level types

### ⏳ Planned
- HTTP notifier implementation
- Working examples for different deployment scenarios
- Comprehensive test suite
- Documentation and examples

## Deployment Scenarios

### Local Development/Testing
```json
{
  "queue": { "type": "in_memory" },
  "mailbox": { "storage": { "type": "in_memory" } }
}
```

### Tauri Desktop App
```json
{
  "queue": {
    "type": "yaque",
    "path": "./data/queues",
    "stream": "llm_tasks"
  },
  "mailbox": { "storage": { "type": "in_memory" } }
}
```

### Cloud Service
```json
{
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
      "base_url": "https://api.example.com/webhooks/task-complete"
    }
  }
}
```

## Dependencies

```toml
[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"], optional = true }
serde_json = { version = "1", optional = true }
thiserror = "1"
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time"], optional = true }
sea-orm = { version = "0.12", features = ["macros", "runtime-tokio-rustls", "sqlx-postgres"], optional = true }
reqwest = { version = "0.12", features = ["json", "rustls-tls"], optional = true }

[features]
default = ["native"]
native = ["tokio"]
postgres = ["sea-orm"]
pgmq = ["postgres"]
yaque = []
config = ["serde", "serde_json"]
```

## Benefits

1. **Prevents Resource Exhaustion**: No more GPU OOM crashes from too many concurrent inference tasks
2. **Work Preservation**: Persistent queues ensure expensive AI computations survive restarts
3. **Client Resilience**: Mailbox system handles disconnected clients gracefully
4. **Environment Flexibility**: Same scheduling logic works across desktop, cloud, and web deployments
5. **Configuration-Driven**: Change deployment characteristics without code changes
6. **Extensible**: Clean trait abstractions allow adding new backends easily

This crate serves as the foundational scheduling infrastructure for the entire Prometheus AI platform, providing consistent resource management and work coordination across all deployment scenarios.