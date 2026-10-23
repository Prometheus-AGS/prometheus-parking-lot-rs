# Implementation Plan: `prometheus_parking_lot`

This document enumerates the step-by-step implementation plan for the **prometheus_parking_lot** crate. The plan is divided into **6 Phases**, moving from the core domain logic to specific infrastructure adapters and finally configuration integration.

---

## Phase 0: Project Scaffold & Dependencies

**Goal:** Initialize the crate structure, set up feature flags, and establish the clean architecture directories.

- [ ] **Initialize Crate**
    - Run `cargo new prometheus_parking_lot --lib`.
- [ ] **Configure `Cargo.toml`**
    - Add `[features]` section: `default = ["native"]`, `native`, `postgres`, `pgmq`, `yaque`, `config`.
    - Add dependencies: `async-trait`, `thiserror`, `tracing`.
    - Add optional dependencies: `tokio`, `serde`, `serde_json`, `sea-orm`, `reqwest`, `yaque`.
- [ ] **Create Directory Structure**
    - Create `src/core/`, `src/infra/`, `src/config/`, `src/builders/`, `src/util/`.
    - Create `mod.rs` files in each subdirectory to export modules.
- [ ] **Define Utility Types**
    - Create `src/util/clock.rs` with a `Clock` trait (methods: `now()`, `now_ms()`) and a `SystemClock` impl. This is crucial for testable timeouts.

---

## Phase 1: The Core Domain (Pure Rust)

**Goal:** Implement the "Parking Lot" algorithm and traits without touching any database or external IO.

- [ ] **Define Domain Types (`src/core/types.rs`)**
    - Implement `TaskId`, `TenantId`, `MailboxKey`.
    - Implement `ResourceKind`, `ResourceCost`, `Priority`.
    - Implement `TaskMetadata` (include `created_at_ms`, `deadline_ms`).
    - Implement `TaskStatus` and `MailboxMessage`.
- [ ] **Define Traits (`src/core/`)**
    - `src/core/task.rs`: Define `TaskPayload` marker trait and `TaskExecutor` async trait.
    - `src/core/queue.rs`: Define `TaskQueue` trait (enqueue, dequeue_next, prune).
    - `src/core/mailbox.rs`: Define `MailboxStorage` and `MailboxNotifier` traits.
    - `src/core/runtime.rs`: Define `Spawn` trait.
- [ ] **Implement Core Scheduler (`src/core/scheduler.rs`)**
    - Define `PoolConfig` struct.
    - Create `ResourcePool` struct with `Arc<Mutex<Inner>>`.
    - **Logic Implementation:**
        - Implement `submit()`: Check `active_units + cost <= max_units`. If yes, run; else, enqueue.
        - Implement `on_task_finished()`: Decrement active units, then loop `dequeue_next()` to wake up pending tasks that fit.
- [ ] **Implement In-Memory Adapters (MVP)**
    - `src/infra/queue/in_memory.rs`: Implement a `BinaryHeap`-based priority queue.
    - `src/infra/mailbox/in_memory.rs`: Implement a `HashMap`-based mailbox.
    - `src/infra/runtime/tokio.rs`: Implement `TokioSpawner`.
- [ ] **Unit Testing**
    - Create `tests/scheduler_basic.rs`.
    - Test: Submit more tasks than capacity allows; ensure excess tasks are queued.
    - Test: Finish a task; ensure the next queued task starts automatically.

---

## Phase 2: Postgres & SeaORM Implementation

**Goal:** Enable persistent queuing and mailbox storage for cloud/server environments.

- [ ] **SQL Migrations**
    - Create `migrations/` folder.
    - Write SQL for `pl_queue_jobs` (columns: id, queue_name, priority, payload, metadata, locked_by).
    - Write SQL for `pl_mailbox_messages` (columns: tenant, user_id, status, payload).
- [ ] **SeaORM Entities**
    - Create `src/infra/entities/pl_queue_jobs.rs`.
    - Create `src/infra/entities/pl_mailbox_messages.rs`.
- [ ] **Implement SeaOrmQueue (`src/infra/queue/sea_orm.rs`)**
    - Implement `enqueue`: Insert into `pl_queue_jobs`.
    - Implement `dequeue_next`: Select `ORDER BY priority DESC, created_at ASC LIMIT 1` (consider `FOR UPDATE SKIP LOCKED` if supported, or simple delete-on-read for v1).
- [ ] **Implement SeaOrmMailbox (`src/infra/mailbox/sea_orm.rs`)**
    - Implement `store`: Insert into `pl_mailbox_messages`.
    - Implement `fetch`: Select by tenant/user/session.
- [ ] **Integration Test**
    - Add a test requiring a Postgres connection string that verifies data persists after the `ResourcePool` is dropped and recreated.

---

## Phase 3: Pgmq & Yaque Adapters

**Goal:** Implement the specialized backends for High-Scale Cloud (Pgmq) and Desktop/Local (Yaque).

- [ ] **Implement PgmqQueue (`src/infra/queue/pgmq.rs`)**
    - Gate behind `feature = "pgmq"`.
    - Implement `enqueue`: Call `pgmq.send`.
    - Implement `dequeue_next`: Call `pgmq.read`, parse JSON, then call `pgmq.delete`.
- [ ] **Implement YaqueQueue (`src/infra/queue/yaque.rs`)**
    - Gate behind `feature = "yaque"`.
    - Manage the `yaque::Sender` and `yaque::Receiver` handles (likely wrapped in a `Mutex`).
    - Implement serialization of `TaskMetadata` + `Payload` into bytes for Yaque.
    - **Crucial:** Ensure the file lock is handled gracefully (Yaque ensures only one process opens the queue).

---

## Phase 4: Mailbox Notification System

**Goal:** separate storage from notification (e.g., Webhooks).

- [ ] **Implement HTTP Notifier (`src/infra/mailbox/http_notifier.rs`)**
    - Gate behind `feature = "config"` (requires reqwest).
    - Implement `MailboxNotifier` trait.
    - Logic: POST the `MailboxMessage` to `base_url`.
- [ ] **Refine Mailbox Composition**
    - Ensure `StorageWithNotifierMailbox` (in core) correctly calls `storage.store()` followed by `notifier.notify()`.

---

## Phase 5: Configuration & Builders

**Goal:** Make the system usable via `scheduler-config.json`.

- [ ] **Define Config Models (`src/config/model.rs`)**
    - Create structs deriving `Deserialize`: `SchedulerConfig`, `PoolConfig`, `QueueConfig` (enum), `MailboxConfig`.
- [ ] **Implement Queue Builder (`src/builders/queue_builder.rs`)**
    - Create function `build_task_queue_from_config`.
    - Logic: Match on config enum (InMemory, Postgres, Pgmq, Yaque) -> instantiate specific struct.
- [ ] **Implement Mailbox Builder (`src/builders/mailbox_builder.rs`)**
    - Logic: Check if `notifier` config exists. If yes, wrap storage in `StorageWithNotifierMailbox`.
- [ ] **Implement Pool Builder (`src/builders/pool_builder.rs`)**
    - Create `build_pools_from_scheduler_config`.
    - Iterate over the map of pools, instantiating the specific executor factory for each named pool.

---

## Phase 6: Polish & Release

**Goal:** Finalize the crate for consumption by other projects.

- [ ] **Export Public API (`src/lib.rs`)**
    - Re-export `ResourcePool`, `SchedulerConfig`, traits, and builders.
    - Hide internal implementation details of infra unless explicitly needed.
- [ ] **Create Examples**
    - `examples/local_tauri.rs`: Uses Yaque + InMemoryMailbox.
    - `examples/cloud_server.rs`: Uses Postgres + Pgmq + HttpNotifier.
- [ ] **Documentation**
    - Add doc comments (`///`) to all public structs.
    - Ensure `README.md` explains the "Why" (VRAM constraints) and the "How" (Config example).
- [ ] **Final `cargo check` & `cargo test`**
    - Run tests with all features enabled: `cargo test --all-features`.
