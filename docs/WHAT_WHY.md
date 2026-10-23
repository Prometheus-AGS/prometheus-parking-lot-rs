Based on the conversation provided, here is a detailed explanation of what you are building and why.

### 1. The "What": `prometheus_parking_lot` Crate

You are building a **standalone, unified Rust crate** named `prometheus_parking_lot`.

This crate acts as a **smart task scheduler** (specifically implementing a "parking lot" pattern) designed to sit between your application's requests and its heavy computation resources (like AI Agents, LLMs, or GPUs).

**Core Functions of the Crate:**
*   **Traffic Control:** It accepts tasks (like "Run this LLM prompt") and decides whether to run them immediately or queue them based on available capacity (e.g., "I only have 24GB VRAM").
*   **The "Parking Lot":** When capacity is full, tasks wait in a prioritized queue rather than failing or crashing the system.
*   **Wake-up Logic:** As soon as a running task finishes and frees up resources, the scheduler automatically wakes up the next highest-priority task from the queue.
*   **Mailbox Delivery:** If a client (user/frontend) disconnects while a long task is running, the result is saved in a "Mailbox" so they can retrieve it later, ensuring no work is wasted.
*   **Configuration-Driven:** The behavior (Queue type, Capacity limits) is defined in a JSON config file (`config::SchedulerConfig`), allowing the same code to run in different environments without recompiling.

### 2. The "Why": Solving the AI Resource Problem

You are building this because standard async programming (like `tokio::spawn`) is insufficient for AI workloads.

1.  **Resource Constraints (VRAM vs. RAM):**
    *   *Standard approach:* In typical web servers, you can spawn thousands of tasks because they mostly wait on Network I/O.
    *   *Your problem:* AI Agents consume massive amounts of **GPU VRAM**. If you try to run 5 concurrent 8GB models on a 24GB card, your application will crash (OOM).
    *   *Solution:* You need a system that explicitly counts "resource units" and refuses to run more tasks than the hardware can handle.

2.  **Durability & Persistence:**
    *   *Standard approach:* If you restart a server, in-memory tasks are lost.
    *   *Your problem:* AI tasks are expensive and time-consuming. You don't want to lose the queue just because the server rebooted or the app crashed.
    *   *Solution:* **Durable lease queues** on **Postgres** (`PgLeaseQueue`) for the cloud and an embedded **redb** file (`RedbLeaseQueue`) for desktop and local use. A pool configured on either backend is built as a `DurableResourcePool` (cr-49) and keeps accepted work across restarts and crashes. `PgmqLeaseQueue` (cr-30) implements the same queue contract over pgmq, but configuration cannot select it yet and durable pools are validated only on `PgLeaseQueue` and redb.

3.  **Unified Architecture (Write Once, Run Anywhere):**
    *   *Your problem:* You are deploying "Prometheus" agents in vastly different environments: Cloud servers, Desktop apps (Tauri), and potentially Web browsers.
    *   *Solution:* By abstracting the "Queue" and "Mailbox" behind traits, the **same scheduler logic** works everywhere. You just swap the backend config:
        *   **Cloud:** Uses Postgres for high-scale queuing.
        *   **Desktop:** Uses an embedded redb file for local queuing without needing a database server.

### 3. Technical Architecture Breakdown

You have moved through several iterations to arrive at a **Feature-Based Clean Architecture**:

*   **Core (Domain Layer):**
    *   Contains the logic for the "Parking Lot" algorithm.
    *   Pure Rust, no dependencies on databases or web frameworks.
    *   Defines the rules: `Capacity > Usage ? Run : Queue`.

*   **Infrastructure (Adapters):**
    *   **redb (embedded):** `EmbeddedStore` opens one crash-safe redb file holding `RedbLeaseQueue` and `RedbResultMailbox`, for the **Tauri/Desktop** use case (cr-28, ADR 0004). It allows persistent queuing without requiring the user to install Postgres. The same store is exposed to Kotlin and Swift through UniFFI bindings (cr-44).
    *   **Postgres:** `PgLeaseQueue` (cr-26) and `PgResultMailbox` (cr-27) on raw sqlx, for **Cloud** deployments that already run a database; `PgmqLeaseQueue` (cr-30) is the same queue contract over pgmq. Tested on PostgreSQL 17 and 18 (cr-36).

*   **Builders & Config:**
    *   A single scheduler configuration file (`config::SchedulerConfig`, examples in `examples/configs/`) declares the pools and opens the durable backends.
    *   Moving a pool from in-memory to a durable backend is a configuration change (durable dispatch, cr-49): a pool whose `queue` and `mailbox` name a Postgres or redb backend persists every accepted task before `submit` returns, extends its lease while it waits or runs, and acks it only after the outcome is stored, so a crash at any point re-runs the task and leaves exactly one result. The executor can still run twice for one task, so its side effects must be idempotent.

### Summary
You are professionalizing your AI Agent infrastructure. Instead of ad-hoc task spawning, you are creating a **dedicated scheduling engine** that treats GPU/CPU availability as a hard constraint, ensures tasks are never lost (persistence), and guarantees results are delivered even if the user goes offline (mailbox).
