Based on the conversation provided, here is a detailed explanation of what you are building and why.

### 1. The "What": `prometheus_parking_lot` Crate

You are building a **standalone, unified Rust crate** named `prometheus_parking_lot`.

This crate acts as a **smart task scheduler** (specifically implementing a "parking lot" pattern) designed to sit between your application's requests and its heavy computation resources (like AI Agents, LLMs, or GPUs).

**Core Functions of the Crate:**
*   **Traffic Control:** It accepts tasks (like "Run this LLM prompt") and decides whether to run them immediately or queue them based on available capacity (e.g., "I only have 24GB VRAM").
*   **The "Parking Lot":** When capacity is full, tasks wait in a prioritized queue rather than failing or crashing the system.
*   **Wake-up Logic:** As soon as a running task finishes and frees up resources, the scheduler automatically wakes up the next highest-priority task from the queue.
*   **Mailbox Delivery:** If a client (user/frontend) disconnects while a long task is running, the result is saved in a "Mailbox" so they can retrieve it later, ensuring no work is wasted.
*   **Configuration-Driven:** The behavior (Queue type, Capacity limits) is defined in a simple JSON/YAML config file, allowing the same code to run in different environments without recompiling.

### 2. The "Why": Solving the AI Resource Problem

You are building this because standard async programming (like `tokio::spawn`) is insufficient for AI workloads.

1.  **Resource Constraints (VRAM vs. RAM):**
    *   *Standard approach:* In typical web servers, you can spawn thousands of tasks because they mostly wait on Network I/O.
    *   *Your problem:* AI Agents consume massive amounts of **GPU VRAM**. If you try to run 5 concurrent 8GB models on a 24GB card, your application will crash (OOM).
    *   *Solution:* You need a system that explicitly counts "resource units" and refuses to run more tasks than the hardware can handle.

2.  **Durability & Persistence:**
    *   *Standard approach:* If you restart a server, in-memory tasks are lost.
    *   *Your problem:* AI tasks are expensive and time-consuming. You don't want to lose the queue just because the server rebooted or the app crashed.
    *   *Solution:* You are implementing **Persistent Queues** using **Postgres/Pgmq** (for Cloud) and **Yaque** (for Desktop/Local), ensuring the queue survives restarts.

3.  **Unified Architecture (Write Once, Run Anywhere):**
    *   *Your problem:* You are deploying "Prometheus" agents in vastly different environments: Cloud servers, Desktop apps (Tauri), and potentially Web browsers.
    *   *Solution:* By abstracting the "Queue" and "Mailbox" behind traits, the **same scheduler logic** works everywhere. You just swap the backend config:
        *   **Cloud:** Uses Postgres for high-scale queuing.
        *   **Desktop:** Uses Yaque (file-backed) for local queuing without needing a database server.

### 3. Technical Architecture Breakdown

You have moved through several iterations to arrive at a **Feature-Based Clean Architecture**:

*   **Core (Domain Layer):**
    *   Contains the logic for the "Parking Lot" algorithm.
    *   Pure Rust, no dependencies on databases or web frameworks.
    *   Defines the rules: `Capacity > Usage ? Run : Queue`.

*   **Infrastructure (Adapters):**
    *   **Yaque:** A Rust-native, file-based queue optimized for the **Tauri/Desktop** use case. It allows persistent queuing without requiring the user to install Postgres.
    *   **Postgres / Pgmq:** An adapter for **Cloud** deployments that leverages your existing database for queuing and locking.
    *   **SeaORM:** Used to interact with custom tables for the "Mailbox" (storage of results).

*   **Builders & Config:**
    *   You designed a system where a single `scheduler-config.json` file determines which adapters are used.
    *   This allows you to change a deployment from "In-Memory" to "Postgres" just by changing a text file, not code.

### Summary
You are professionalizing your AI Agent infrastructure. Instead of ad-hoc task spawning, you are creating a **dedicated scheduling engine** that treats GPU/CPU availability as a hard constraint, ensures tasks are never lost (persistence), and guarantees results are delivered even if the user goes offline (mailbox).
