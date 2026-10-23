# Architectural Design Document: `prometheus_parking_lot`

## 1. Executive Summary

`prometheus_parking_lot` is a specialized Rust library designed to solve the "resource-constrained scheduling problem" inherent in modern AI agent architectures. Whether running local Large Language Models (LLMs) on a desktop with limited VRAM (Video RAM), or managing high-throughput inference in a multi-tenant cloud environment, AI agents require strict concurrency control that standard async runtimes (like Tokio) do not inherently provide.

This library introduces a **Parking Lot Scheduler**: a dedicated layer that decouples *task submission* from *task execution*. It enforces strict resource limits (e.g., "Max 12GB VRAM"), queues excess load, handles persistent task durability across restarts, and manages result delivery for disconnected clients (e.g., mobile apps or CLI tools). By abstracting the backing infrastructure (in-memory, Postgres, embedded redb), it allows the same agent code to run unmodified in embedded Tauri apps, web servers, and scalable cloud clusters.

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
        Mailbox -.->|Notify, StorageMailbox only| Notifier[Notifier]
    end

    subgraph "Infrastructure Adapters"
        Queue -.->|Persist| DB[(Postgres / redb / Mem)]
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
        +u64 sequence
        +MailboxKey mailbox_ref
    }

    class ScheduledTask~P~ {
        +TaskMetadata meta
        +P payload
    }

    class TaskQueue~P~ {
        <<interface>>
        +enqueue(task)
        +dequeue()
        +prune_expired()
    }

    class Mailbox~T~ {
        <<interface>>
        +deliver(key, result)
        +consume(key)
    }

    class TaskExecutor~P,T~ {
        <<interface>>
        +execute(payload, meta) Result~T, TaskError~
    }

    ResourcePool --> PoolConfig
    ResourcePool --> TaskQueue
    ResourcePool --> Mailbox
    ResourcePool --> TaskExecutor
    TaskQueue ..> ScheduledTask
    ScheduledTask *-- TaskMetadata
```

### 4.2 Component Descriptions

1. **ResourcePool:** The brain of the system. All shared state sits in one `Arc<Inner>`. Admission goes through the core `Dispatcher`, which owns the parked queue and a `UnitSemaphore` (one packed `AtomicU64` claimed by compare-and-swap), so admission, parking and waking happen under one lock (cr-19). When a job arrives the pool stamps its submission sequence and computes its cost: it starts immediately only if nothing is parked ahead of it and the cost fits, otherwise it is parked. When a job finishes (value or `TaskError`), the pool releases its units, delivers `Completed` or `Failed(TaskError)` to the mailbox, and starts queued jobs in priority-then-FIFO order while the head fits. Time is read through a `Clock`. The contracts (error taxonomy, fallible executors, clock, sequence, cost vector) are recorded in [ADR 0002](adr/0002-core-contracts.md).
2. **TaskQueue (Trait):** Abstraction for ordered job storage. Implementations include:
   - `InMemoryQueue`: High-performance non-durable `BinaryHeap` with O(log n) enqueue/dequeue operations. It is the only `TaskQueue`; the durable queues implement `LeaseQueue` (below).
   - Durable `LeaseQueue` backends: `PgLeaseQueue` (cr-26, raw sqlx, `FOR UPDATE SKIP LOCKED`, table `pl_lease_queue`), `PgmqLeaseQueue` (cr-30, the pgmq extension through plain SQL, one pgmq queue per priority) and `RedbLeaseQueue` (cr-28, embedded redb file, for desktop and single-node use; ADR 0004).
3. **Mailbox (Trait):** Abstraction for result storage. It handles the "late pickup" pattern where a mobile client might disconnect before an LLM finishes generation. `InMemoryMailbox` is the in-process `Mailbox`; the durable `ResultMailbox` backends are `PgResultMailbox` (cr-27, table `pl_result_mailbox`) and `RedbResultMailbox` (cr-28). `StorageMailbox<S, N>` stores first and notifies off the delivery path with bounded retries (cr-34); the only built-in notifier is `NoopNotifier`.
   - **Durable backend contracts (ADR 0003):** `LeaseQueue` (async enqueue with idempotent acceptance, `lease` with fenced `LeaseToken`s, `ack`, `nack` requeue/dead-letter, `extend` heartbeat, redelivery on lease expiry) and `ResultMailbox` (async, first-write-wins on the idempotency key remembered past consume, consume-once, TTL), with `Notifier` and the `StorageMailbox<S, N>` composition. Every backend passes the conformance suite published by the core crate's `conformance` feature; `InMemoryLeaseQueue` and `InMemoryResultMailbox` are the reference implementations. The synchronous `TaskQueue`/`Mailbox` above remain the in-process contracts of the in-memory `ResourcePool`; its durable mode, `DurableResourcePool` (cr-49), runs on these. See [ADR 0003](adr/0003-backend-contracts.md).
4. **TaskExecutor (Trait):** The user-supplied logic. This is where the actual Agent code (e.g., "Run Llama-3 inference") lives. It returns `Result<T, TaskError>`; a failure is recorded, never mistaken for a result.

------

## 5. Flow & Scenarios

### 5.1 Scenario A: Local LLM Inference (Tauri Desktop)

*Context:* User requests a summary of a PDF. The local GPU has 8GB VRAM. A background indexing job is already running.

Code snippet

```
sequenceDiagram
    participant UI as Tauri UI
    participant Pool as ResourcePool
    participant Queue as Parked queue (in memory)
    participant Exec as LLM Executor
    participant GPU as GPU Resource

    UI->>Pool: submit(SummarizeTask, cost=4GB)
    Note over Pool: Active: 6GB (Indexing)<br/>Max: 8GB
    Pool->>Pool: Check Capacity (6+4 > 8)
    Pool->>Queue: enqueue(SummarizeTask)
    Pool-->>UI: Returns TaskID (Queued)

    Note over GPU: Indexing Job Finishes (-6GB)
    Pool->>Pool: Update Active: 0GB
    Pool->>Queue: dequeue()
    Queue-->>Pool: Returns SummarizeTask
    Pool->>Pool: Update Active: 4GB
    Pool->>Exec: execute(SummarizeTask)
    Exec->>GPU: Load Model & Infer
    GPU-->>Exec: Result "Summary..."
    Exec-->>Pool: Finished
    Pool->>Pool: Update Active: 0GB
```

This is the in-memory pool: the parked task is lost if the app quits. To keep it
across a restart, configure the pool on a `redb` backend; it then runs as a
`DurableResourcePool` on a `RedbLeaseQueue` and `RedbResultMailbox`, with the
durable sequence shown in Scenario B.

### 5.2 Scenario B: Cloud Multi-Tenant Gateway

*Context:* High-load web server using Postgres. Multiple users submitting jobs simultaneously. The pool's `queue` and `mailbox` name a `postgres` backend, so it runs as a `DurableResourcePool` (cr-49) on `PgLeaseQueue` and `PgResultMailbox`.

Code snippet

```
sequenceDiagram
    participant Client as Web Client
    participant Pool as DurableResourcePool
    participant PG as PgLeaseQueue
    participant Box as PgResultMailbox

    Client->>Pool: submit(ChatTask)
    Pool->>PG: enqueue(ChatTask) (stored before submit returns)
    Pool-->>Client: Queued

    loop Dispatcher, while a unit is free
        Pool->>PG: lease(1, lease_for) (FOR UPDATE SKIP LOCKED)
        PG-->>Pool: ChatTask + LeaseToken
        Pool->>PG: extend(token) every third of the lease (heartbeat)
        Pool->>Pool: execute() under the budget
        Pool->>Box: deliver(Result, idempotency key)
        Pool->>PG: ack(token) only after the delivery is stored
    end

    Note over Client: Client Disconnects<br/>(Timeout/Network)

    Client->>Box: consume(MailboxKey) (Reconnects later)
    Box-->>Client: Result payload, removed
```

If the process dies at any point before the ack, the task stays leased, its
lease expires and the queue redelivers it. The mailbox keeps the first write
for the idempotency key, so the client sees exactly one result; the executor
may run twice, so executors with external side effects must be idempotent.

------

## 6. Implementation Strategy: Clean Architecture

The codebase is a Cargo workspace organized so the core logic never depends on specific databases or runtimes. The crate map, dependency rules and feature mapping are recorded in [ADR 0001](adr/0001-workspace-split.md).

| **Layer**       | **Crate (directory)**                                       | **Responsibilities**                                         | **Dependencies**           |
| --------------- | ----------------------------------------------------------- | ------------------------------------------------------------ | -------------------------- |
| **Domain**      | `prometheus-parking-lot-core` (`crates/parking-lot-core`)   | Contracts (`LeaseQueue`, `ResultMailbox`, `Notifier`, `TaskQueue`, `Mailbox`, `Spawn`, executors), task types, errors, hooks, in-memory reference backends, the backend conformance suite (feature `conformance`), `WorkerPoolConfig`. | No runtime, IO, DB or network crate |
| **Runtime**     | `prometheus-parking-lot-runtime-tokio` (`crates/parking-lot-runtime-tokio`) | `ResourcePool`, `DurableResourcePool`, native `WorkerPool`, shutdown, cancellation, `TokioSpawner`. | Core, Tokio |
| **Adapters**    | `prometheus-parking-lot-pg`, `-embedded`, `-wasm` (`crates/`) | `PgLeaseQueue`, `PgmqLeaseQueue`, `PgResultMailbox`, Postgres audit sink and migrations; redb `EmbeddedStore`, `RedbLeaseQueue`, `RedbResultMailbox`; WASM `WorkerPool`. | Core + raw sqlx 0.9; redb + Tokio blocking pool; `wasm-bindgen-futures` (wasm, no Tokio) |
| **Bindings**    | `prometheus-parking-lot-ffi` (`crates/parking-lot-ffi`, not published) | UniFFI 0.32 Kotlin and Swift bindings over the embedded redb queue and mailbox (cr-44); see `bindings/README.md`. | Embedded crate, UniFFI |
| **Facade**      | `prometheus_parking_lot` (repository root)                  | Re-exports behind features; `config/`, `builders/`, `runtime/api`. | All of the above, by feature |

### 6.1 Configuration-Driven Design

Pools and durable backends are described in a JSON configuration
(`config::SchedulerConfig`) and built by
`builders::build_pools_from_scheduler_config`. The schema rejects unknown
fields. Durable backends (Postgres, redb) are opened from their own sections.
A pool whose `queue` and `mailbox` are `"in_memory"` parks tasks and delivers
results in memory; a pool whose `queue` and `mailbox` both name a backend runs
as a `DurableResourcePool` on that backend's `LeaseQueue` and `ResultMailbox`
(durable dispatch, cr-49): persist before accept, lease with a heartbeat,
deliver, then ack. Each backend drives at most one pool. A `"type": "pgmq"`
backend opens a `PgmqLeaseQueue` for the application to use directly; no pool
runs on it, because pool-level crash durability on pgmq is not claimed
(scenario S-18).

```json
{
  "backends": {
    "local": { "type": "redb", "path": "./data/parking-lot.redb", "max_depth": 1000, "lease_secs": 300 },
    "cloud": { "type": "postgres", "url": "postgres://parking_lot@db:5432/parking_lot",
               "queue_name": "inference", "max_depth": 10000, "lease_secs": 300,
               "listen": true, "migrate": true }
  },
  "pools": {
    "local_agent": { "max_units": 8, "max_queue_depth": 200, "default_timeout_secs": 120,
                     "queue": "in_memory", "mailbox": "in_memory" },
    "cloud_agent": { "max_units": 100, "max_queue_depth": 5000, "default_timeout_secs": 600,
                     "queue": "in_memory", "mailbox": "in_memory" }
  }
}
```

Complete examples, loaded and built by the test suite: `examples/configs/`.

------

## 7. Performance Characteristics

The library is optimized for high-throughput scenarios using `parking_lot` crate primitives and lock-free data structures.

### 7.1 Synchronization Primitives

The timings below were measured on 0.2, before the dispatcher and semaphore
(cr-19) replaced the 0.2 primitives; they have not been re-measured.

| Component | Primitive on `main` | 0.2 measurement |
|-----------|-----------|-------------|
| Capacity tracking | `UnitSemaphore`: one packed `AtomicU64` + CAS (units and permits together) | ~700ps per operation (0.2 `AtomicU32` CAS) |
| Admission, parking and waking | `Dispatcher` under one `parking_lot::Mutex` | ~888ns lock/unlock |
| Mailbox access | `parking_lot::Mutex` | ~888ns lock/unlock |
| Native worker hand-off | crossbeam channel of admitted tasks | not measured (0.2 used a `Condvar`, ~2.4µs `notify_one`) |

### 7.2 Queue Performance

The `InMemoryQueue` uses a `BinaryHeap` for O(log n) priority queue operations:

| Queue Size | Enqueue+Dequeue Time | Throughput |
|------------|---------------------|------------|
| 100 | ~12µs | 8.4M elem/s |
| 1,000 | ~143µs | 7.0M elem/s |
| 10,000 | ~1.75ms | 5.7M elem/s |

### 7.3 Design Decisions

1. **Lock-free capacity tracking:** the `UnitSemaphore` claims units and permits in one CAS on a packed `AtomicU64`, so both limits are taken together or not at all.
2. **Fine-grained locking:** Queue and mailbox have separate mutexes, reducing lock contention.
3. **BinaryHeap vs VecDeque:** Priority queues use heap for O(log n) vs O(n log n) sorted insertion.
4. **No lost wakeups:** admission, parking and waking happen under the dispatcher's lock, and completion starts the parked work the freed units admit. An idle durable pool is the exception: it polls its queue (500 ms by default) for work enqueued by another process.

------

## 8. Importance to AI Agent Design

1. **Safety Valve:** AI models are heavy. Without `prometheus_parking_lot`, a burst of 10 requests could crash a local agent by exhausting VRAM. This library turns crashes into queues.
2. **User Experience:** A pool configured on a durable backend (embedded redb on the desktop, Postgres in the cloud) keeps accepted requests if the app crashes or updates. On restart, tasks that were waiting run, and tasks that were running are run again from the start; the client still gets exactly one result.
3. **Unified Platform:** The desktop app (an embedded redb file) and the cloud SaaS (Postgres) use the same durable pool type, `DurableResourcePool`, and the backend is chosen in configuration. Moving a pool from in-memory to a durable backend is not transparent, though: the builder returns it in `BuiltScheduler::durable_pools` as a `ConfiguredDurablePool` instead of a `ConfiguredPool`, results are read from an async `ResultMailbox` instead of the synchronous `Mailbox`, and shutdown hands unfinished work back to the queue instead of recording it as cancelled. The library abstracts this complexity entirely.
4. **High Performance:** Lock-free capacity tracking and optimized queue operations ensure the scheduler itself doesn't become a bottleneck, even under heavy load.