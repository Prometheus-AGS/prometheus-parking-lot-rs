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

Every number in this section was measured by the benchmark suites in `benches/`
(cr-40; `benches/README.md` describes the workloads and the method).

**Machine and run.** Apple M1 Max (8 performance + 2 efficiency cores), 64 GB,
macOS 26.7.2, internal SSD for `$TMPDIR`; rustc 1.99.1, the `bench` profile;
2026-10-23, commit 6824e83 plus the cr-40 suites. The machine was shared with other
builds: load average 20 to 50 during the runs. Two full runs of

```bash
cargo bench --all-features --locked \
  --bench components --bench throughput --bench latency --bench utilization -- --noplot
```

and two runs at the CI configuration (`--warm-up-time 1 --measurement-time 3
--sample-size 20`). Where two values are given they are the two full runs; a single
value means the runs agreed to the shown precision.

**CI numbers differ.** The `Benchmarks` CI job runs on a GitHub-hosted 4-vCPU x86_64
Linux runner, whose CPU and fsync cost differ from this machine; its results (job
summary and the `bench-results-<sha>` artifact) are the reproducible record per commit,
and they are not expected to match the figures below. Treat these figures as one
machine's evidence of orders of magnitude and ratios, not as guarantees.

### 7.1 Component costs (`components`)

One thread, no contention. Mean per operation.

| Operation | Primitive | Time |
|---|---|---|
| Claim and release 1 unit | `UnitSemaphore`: one packed `AtomicU64`, CAS (units and permits together) | 14.5 ns |
| Claim and release a vector with a per-kind limit (cr-23) | packed CAS plus the kind lock | 24 to 25 ns |
| Submit that starts at once, then complete | `Dispatcher` under one `parking_lot::Mutex` | 35 to 37 ns |
| Submit that parks, then the completion that starts it | `Dispatcher` | 109 ns |
| Emit a lifecycle event, no hook registered | `HookBus` (one atomic load) | 0.55 ns |
| Emit a lifecycle event to one no-op hook | `HookBus` bounded channel | 53 to 54 ns |
| 100 tasks through a `ResourcePool`, no hook / one no-op hook | whole cycle | 172 to 183 µs / 291 µs |
| `InMemoryQueue` enqueue + dequeue, 100 / 1,000 / 10,000 tasks | `BinaryHeap`, O(log n) | 14 to 15 µs / 191 µs / 2.25 to 2.29 ms (6.5 to 7.0 / 5.2 / 4.4 Melem/s) |
| Same with four priorities mixed, 1,000 tasks | | 244 to 253 µs (3.9 to 4.1 Melem/s) |
| `InMemoryMailbox` deliver, 1,000 messages | `HashMap` | 143 to 146 µs (6.9 to 7.0 Melem/s) |
| `InMemoryMailbox` lookup of 5,000 keys | | 220 to 224 µs |

A registered hook costs about 1.2 µs per task in a pool cycle (three events per task,
submitted, started and completed, through the delivery channel and thread), not the
53 ns of a single emission.

### 7.2 Throughput (`throughput`)

Batches through a pool of 16 units, executor doing no work, client on a 4-worker tokio
runtime; a batch ends when every result is readable by the client (stored in the
mailbox, or returned by `retrieve_async`). Unit tasks cost 1 unit; mixed tasks 1 to 4.

| Pool | Unit tasks | Mixed costs |
|---|---|---|
| `ResourcePool` (in-memory queue and mailbox) | 470,000 tasks/s | 408,000 to 436,000 tasks/s |
| native `WorkerPool` (16 threads, a waiter task per result) | 219,000 to 234,000 tasks/s | 201,000 to 212,000 tasks/s |
| `DurableResourcePool`, in-memory lease queue and result mailbox | 175,000 to 193,000 tasks/s | |
| `DurableResourcePool`, redb (`embedded`), fsync per commit | 43 to 44 tasks/s | |

The redb pool is bound by storage, not by the scheduler: every task is at least four
durable commits (persist on submit, lease, deliver, ack), and on this Mac each commit
is a full fsync. The in-memory durable pool shows what the durable dispatch path
itself costs.

### 7.3 Latency (`latency`)

Microseconds from `submit` to the executor starting (start) and to the result being
readable (result). Median of 5 rounds; the two values are the two full runs.

**Light load** (one task in flight, executor doing no work):

| Pool | Start p50 | Start p99 | Result p50 | Result p99 |
|---|---|---|---|---|
| `ResourcePool` | 5.2 to 6.0 | 18 to 32 | 5.6 to 6.5 | 21 to 35 |
| native `WorkerPool` | 5.2 to 5.7 | 20 to 70 | 11.3 to 11.7 | 38 to 116 |
| `DurableResourcePool`, in-memory backends | 6.2 to 10.0 | 25 to 74 | 7.1 to 11.8 | 27 to 87 |
| `DurableResourcePool`, redb | 15,300 to 15,400 | 24,000 to 27,000 | 20,300 to 20,400 | 33,000 to 50,000 |

The p99 values move more between runs than the p50 values: they are where other load
on the machine shows.

**Saturated load** (the whole round submitted at once, many times the 16 units, each
task holding 1 unit for a 1 ms tokio sleep, which the timer turns into 1 to 3 ms): start
latency is then queueing (a task waits for every task ahead of it; p50 117 to 158 ms
for 1,600 tasks in the in-memory pools), so the useful figure is how busy the pool kept
its units from the first submission to the last result:

| Pool | Utilization of 16 units |
|---|---|
| `ResourcePool` | 0.95 to 0.99 |
| native `WorkerPool` | 0.95 to 0.99 |
| `DurableResourcePool`, in-memory backends | 0.83 to 0.91 |
| `DurableResourcePool`, redb | 0.01 |

The durable pool leases one task at a time and holds at most one leased task waiting
for capacity (section "Ordering difference" in the `durable_pool` module docs), which
is where its lower utilization comes from. On redb the client cannot even build a
backlog: each `submit` returns only after its durable enqueue commit, so tasks arrive
about as slowly as the pool serves them (section 7.2: 43 to 44 tasks/s), far below
what 16 units of 1 to 3 ms tasks could run.

### 7.4 Scheduling efficiency (`utilization`)

A virtual-time simulation of the real `Dispatcher` and `UnitSemaphore` (2,000 ticks,
seed `0x5eed`). The results depend only on scheduling decisions, so they are the same
on every machine.

| Workload | Strict order | Skip-ahead, default guard (8) | Large-item wait p50 / max, strict → default |
|---|---|---|---|
| `mixed`: 16 units, about 95% offered | 0.917 | 0.947 | 47 / 73 → 4 / 11 ticks |
| `overload`: 16 units, about 115% offered | 0.928 | 0.955 | 183 / 367 → 158 / 318 ticks |
| `multi`: cost vectors, `GpuVram` limit 8 (the bottleneck), `Cpu` limit 12 | `GpuVram` 0.913, `Cpu` 0.319 | `GpuVram` 0.947, `Cpu` 0.332 | 41 / 90 → 13 / 25 ticks |

Over 100-tick windows the `mixed` workload with the default guard stays between 0.907
and 0.972. The dispatcher's own cost per simulated run (about 3,200 admissions and
completions) is 0.75 to 1.24 ms.

### 7.5 Design Decisions

1. **Lock-free capacity tracking:** the `UnitSemaphore` claims units and permits in one CAS on a packed `AtomicU64`, so both limits are taken together or not at all.
2. **Fine-grained locking:** Queue and mailbox have separate mutexes, reducing lock contention.
3. **BinaryHeap vs VecDeque:** Priority queues use heap for O(log n) vs O(n log n) sorted insertion.
4. **No lost wakeups:** admission, parking and waking happen under the dispatcher's lock, and completion starts the parked work the freed units admit. An idle durable pool is the exception: it polls its queue (500 ms by default) for work enqueued by another process.

Not measured: contention on the semaphore and dispatcher from many threads at once,
memory use, and Postgres-backed pools (the Postgres suites need Docker; see
`benches/README.md`).

------

## 8. Importance to AI Agent Design

1. **Safety Valve:** AI models are heavy. Without `prometheus_parking_lot`, a burst of 10 requests could crash a local agent by exhausting VRAM. This library turns crashes into queues.
2. **User Experience:** A pool configured on a durable backend (embedded redb on the desktop, Postgres in the cloud) keeps accepted requests if the app crashes or updates. On restart, tasks that were waiting run, and tasks that were running are run again from the start; the client still gets exactly one result.
3. **Unified Platform:** The desktop app (an embedded redb file) and the cloud SaaS (Postgres) use the same durable pool type, `DurableResourcePool`, and the backend is chosen in configuration. Moving a pool from in-memory to a durable backend is not transparent, though: the builder returns it in `BuiltScheduler::durable_pools` as a `ConfiguredDurablePool` instead of a `ConfiguredPool`, results are read from an async `ResultMailbox` instead of the synchronous `Mailbox`, and shutdown hands unfinished work back to the queue instead of recording it as cancelled. The library abstracts this complexity entirely.
4. **High Performance:** Lock-free capacity tracking and optimized queue operations ensure the scheduler itself doesn't become a bottleneck, even under heavy load.