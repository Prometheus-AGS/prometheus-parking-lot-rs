# Architectural Design Document: `prometheus_parking_lot`

## 1. Executive Summary

`prometheus_parking_lot` is a specialized Rust library designed to solve the "resource-constrained scheduling problem" inherent in modern AI agent architectures. Whether running local Large Language Models (LLMs) on a desktop with limited VRAM (Video RAM), or managing high-throughput inference in a multi-tenant cloud environment, AI agents require strict concurrency control that standard async runtimes (like Tokio) do not inherently provide.

This library introduces a **Parking Lot Scheduler**: a dedicated layer that decouples *task submission* from *task execution*. It enforces strict resource limits (e.g., "Max 12GB VRAM"), queues excess load, handles persistent task durability across restarts, and manages result delivery for disconnected clients (e.g., mobile apps or CLI tools). By abstracting the backing infrastructure (In-Memory, Postgres, Yaque), it allows the same agent code to run unmodified in embedded Tauri apps, web servers, and scalable cloud clusters.

------

## 2. Architectural Goals

1. **Resource Safety:** Prevent Out-Of-Memory (OOM) crashes by scheduling based on *resource units* (e.g., VRAM estimates) rather than just thread count.
2. **Runtime Agnosticism:** The core logic must operate identically whether running on `tokio` (native), web workers (WASM), or inside a Tauri app.
3. **Durability & Resilience:** Jobs must survive application restarts (via persistent queues) and results must be retrievable if the client disconnects (via mailboxes).
4. **Configuration-Driven:** Infrastructure choices (e.g., "Use Postgres for queueing") should be defined in configuration, not code, enabling seamless deployment across different environments.

------

## 3. High-Level Architecture

The system is architected as a **Hexagonal (Ports and Adapters)** system. The *Core* contains the scheduling logic and domain entities. *Adapters* provide implementations for specific queues, storage engines, and runtimes.

### 3.1 System Context Diagram

This diagram illustrates how `prometheus_parking_lot` sits between the Client Application (Agent) and the underlying infrastructure resources.

Code snippet

```
graph TD
    subgraph "Prometheus AI Platform"
        Client[Agent / Gateway] -->|Submit Job| Pool[Resource Pool]
        Pool -->|Ack / Job ID| Client
        
        Pool -->|Check Capacity| ResourceModel[Cost Model]
        
        subgraph "Parking Lot Core"
            Pool -->|Enqueue (if full)| Queue[Task Queue]
            Pool -->|Dequeue (if free)| Queue
            Pool -->|Execute| Runtime[Async Runtime]
            Runtime -->|Result| Mailbox[Mailbox]
        end
        
        Client -->|Poll Result| Mailbox
        Mailbox -->|Notify| Webhook[External Webhook]
    end

    subgraph "Infrastructure Adapters"
        Queue -.->|Persist| DB[(Postgres / Yaque / Mem)]
        Mailbox -.->|Persist| DB
        Runtime -.->|Spawn| Workers[Threads / WebWorkers]
    end

    style Pool fill:#f9f,stroke:#333,stroke-width:2px
    style Client fill:#ccf,stroke:#333
```

------

## 4. Core Components & Entity Design

The core domain relies on four primary abstractions that decouple the "what" (logic) from the "how" (infrastructure).

### 4.1 Class Diagram (Entity Relationships)

Code snippet

```
classDiagram
    class ResourcePool {
        -PoolConfig config
        -TaskQueue queue
        -Mailbox mailbox
        -TaskExecutor executor
        +submit(task)
        +on_task_finished(result)
    }

    class PoolConfig {
        +u32 max_units
        +usize max_queue_depth
        +Duration timeout
    }

    class TaskMetadata {
        +TaskId id
        +Priority priority
        +ResourceCost cost
        +MailboxKey mailbox_ref
    }

    class ScheduledTask~P~ {
        +TaskMetadata meta
        +P payload
    }

    class TaskQueue~P~ {
        <<interface>>
        +enqueue(task)
        +dequeue_next()
        +prune_expired()
    }

    class Mailbox~T~ {
        <<interface>>
        +deliver(key, result)
        +fetch(key)
    }

    class TaskExecutor~P,T~ {
        <<interface>>
        +execute(payload) Result~T~
    }

    ResourcePool --> PoolConfig
    ResourcePool --> TaskQueue
    ResourcePool --> Mailbox
    ResourcePool --> TaskExecutor
    TaskQueue ..> ScheduledTask
    ScheduledTask *-- TaskMetadata
```

### 4.2 Component Descriptions

1. **ResourcePool:** The brain of the system. It tracks `active_units` (current load). When a job arrives, it calculates `cost`. If `active + cost > max`, the job is sent to the `TaskQueue`. When a job finishes, it pulls from the queue.
2. **TaskQueue (Trait):** Abstraction for ordered job storage. Implementations include:
   - `InMemoryQueue`: Fast, non-durable heap.
   - `SeaOrmQueue`: Durable Postgres table.
   - `YaqueQueue`: Durable local file-based queue (for Desktop).
3. **Mailbox (Trait):** Abstraction for result storage. It handles the "late pickup" pattern where a mobile client might disconnect before an LLM finishes generation.
4. **TaskExecutor (Trait):** The user-supplied logic. This is where the actual Agent code (e.g., "Run Llama-3 inference") lives.

------

## 5. Flow & Scenarios

### 5.1 Scenario A: Local LLM Inference (Tauri Desktop)

*Context:* User requests a summary of a PDF. The local GPU has 8GB VRAM. A background indexing job is already running.

Code snippet

```
sequenceDiagram
    participant UI as Tauri UI
    participant Pool as ResourcePool
    participant Queue as Yaque (File Queue)
    participant Exec as LLM Executor
    participant GPU as GPU Resource

    UI->>Pool: submit(SummarizeTask, cost=4GB)
    Note over Pool: Active: 6GB (Indexing)<br/>Max: 8GB
    Pool->>Pool: Check Capacity (6+4 > 8)
    Pool->>Queue: enqueue(SummarizeTask)
    Pool-->>UI: Returns TaskID (Queued)

    Note over GPU: Indexing Job Finishes (-6GB)
    Pool->>Pool: Update Active: 0GB
    Pool->>Queue: dequeue_next()
    Queue-->>Pool: Returns SummarizeTask
    Pool->>Pool: Update Active: 4GB
    Pool->>Exec: execute(SummarizeTask)
    Exec->>GPU: Load Model & Infer
    GPU-->>Exec: Result "Summary..."
    Exec-->>Pool: Finished
    Pool->>Pool: Update Active: 0GB
```

### 5.2 Scenario B: Cloud Multi-Tenant Gateway

*Context:* High-load web server using Postgres. Multiple users submitting jobs simultaneously.

Code snippet

```
sequenceDiagram
    participant Client as Web Client
    participant Pool as ResourcePool
    participant PG as Postgres (Pgmq)
    participant Box as Mailbox (DB)

    Client->>Pool: submit(ChatTask)
    Pool->>PG: enqueue(ChatTask)
    Pool-->>Client: TaskID

    loop Async Worker Loop
        Pool->>PG: dequeue_next()
        PG-->>Pool: ChatTask
        Pool->>Pool: execute()
        Pool->>Box: deliver(Result)
    end

    Note over Client: Client Disconnects<br/>(Timeout/Network)
    
    Client->>Box: fetch(TaskID) (Reconnects later)
    Box-->>Client: Result payload
```

------

## 6. Implementation Strategy: Clean Architecture

The codebase is organized to ensure the `core` logic never depends on specific databases or runtimes.

| **Layer**       | **Directory**   | **Responsibilities**                                         | **Dependencies**           |
| --------------- | --------------- | ------------------------------------------------------------ | -------------------------- |
| **Domain**      | `src/core/`     | Traits (`TaskQueue`, `Mailbox`), Logic (`ResourcePool`), Types. | None (pure Rust + Serde)   |
| **Config**      | `src/config/`   | Serializable structs for JSON/YAML configuration.            | Serde                      |
| **Adapters**    | `src/infra/`    | Concrete implementations (`SeaOrmQueue`, `YaqueQueue`, `TokioSpawner`). | SQLx, SeaORM, Yaque, Tokio |
| **Composition** | `src/builders/` | Factories that wire Config + Adapters into a `ResourcePool`. | Core, Config, Infra        |

### 6.1 Configuration-Driven Design

Infrastructure is selected at runtime via configuration, enabling the "Write Once, Run Anywhere" capability.

**Example `scheduler-config.json`:**

JSON

```
{
  "pools": {
    "local_agent": {
      "max_units": 8,
      "queue": { "type": "yaque", "path": "./data" },
      "mailbox": { "type": "in_memory" }
    },
    "cloud_agent": {
      "max_units": 100,
      "queue": { "type": "postgres_pgmq", "queue_name": "inference" },
      "mailbox": { "type": "postgres", "table": "results" }
    }
  }
}
```

------

## 7. Importance to AI Agent Design

1. **Safety Valve:** AI models are heavy. Without `prometheus_parking_lot`, a burst of 10 requests could crash a local agent by exhausting VRAM. This library turns crashes into queues.
2. **User Experience:** By utilizing persistent queues (Yaque/Postgres), user requests aren't lost if the app crashes or updates. The agent picks up exactly where it left off upon restart.
3. **Unified Platform:** Developers learn *one* API (`ResourcePool::submit`). They don't need to write separate code for the desktop app (file-based queuing) and the cloud SaaS (Postgres queuing). The library abstracts this complexity entirely.