# prometheus-parking-lot-rs — Code Audit, Platform Review & Next-Phase Plan

**Repository:** `Prometheus-AGS/prometheus-parking-lot-rs`
**Revision audited:** `main` @ `7939b07` ("added postgres backend for queue and mailbox")
**Date:** 2026-10-22
**Author:** Claude (for Travis James)
**How this audit was done:** I read the source, tests, CI, migrations, `docs/`, `.kbd-orchestrator/` and `specs/` directly. **I did not compile or run anything**: the build was blocked in the audit environment because it would have executed dependency build scripts. Run `cargo clippy --all-targets --all-features` and `cargo test --all-features` locally to confirm the build-level findings. Line numbers refer to the audited commit.

---

## Table of contents

1. Executive summary
2. Security: credentials committed to the repo
3. Correctness bugs
4. Patterns that are missing or only half-built
5. Missed optimizations
6. Build, tests and repository hygiene
7. Postgres 18 compatibility
8. Apache Iggy: event propagation and mailbox
9. Rust queue options that have appeared since this crate was built
10. Review of the plans and phases in `docs/` (and `.kbd-orchestrator/`)
11. Recommended next phase
12. Sources

---

## 1. Executive summary

The crate's stated purpose is **resource-aware admission control** ("never exceed 24 GB of VRAM"), **durable parking** of expensive work, and **mailbox delivery** to disconnected clients. As written, it does not reliably deliver any of the three:

- **`WorkerPool`, the primary API, never enforces `max_units`.** Units are counted but never used to gate admission. Priority is ignored.
- **`ResourcePool` enforces units, but can lose tasks and leak capacity.** It has a lost-wakeup race, a re-enqueue path that drops work, and no panic guard.
- **Every Postgres backend panics when called from async code.** They call `Handle::block_on` inside the runtime. The Postgres queue is also at-most-once (delete-on-dequeue), and the Postgres mailbox can't be read back.
- **The file ("Yaque") queue isn't crash-safe** and ignores priority.
- **The hooks, cancellation-capture and shutdown-policy subsystems exist only as types.** None of them is wired into either pool, even though the certification phase is marked 14/14 complete.
- **About 30% of `docs/` describes a different crate**: a `parking_lot` primitives wrapper with `Mutex`/`RwLock`/`Once`/`Condvar` modules that don't exist here.
- **Live agent credentials are committed** (`.codex/auth.json`), and `cargo publish` would ship them to crates.io.

**Recommended direction:**
- Rebuild the scheduling core around a **unit-weighted semaphore with a priority, skip-ahead dispatcher**.
- Make the queue trait **async with lease/ack semantics**.
- Put Postgres on a **single-statement `SKIP LOCKED` lease** that targets PG 17 and 18.
- Use **Apache Iggy for lifecycle-event propagation and mailbox notification**, but **not** as mailbox storage or as the priority work queue.
- For the desktop path, replace the JSONL queue with a **redb/fjall-backed store** (or adopt `rustqueue`).

---

## 2. Security: credentials committed to the repo (fix immediately)

| Item | Detail |
|---|---|
| `.codex/auth.json` | Committed in `ec6b4b4` (2025-12-07). Contains a `tokens` object with 4 entries plus `last_refresh`, which matches the shape of Codex/ChatGPT OAuth tokens. I did not print the values. |
| `.codex/sessions/2025/12/06/rollout-*.jsonl` | 6 lines match `sk-\|api_key\|bearer`. They may only be mentions, but treat them as sensitive. |
| `.codex/history.jsonl`, `.roo/`, `.cursor/`, `.specify/` | Agent tooling state and prompts. Not secret as far as I can tell, but none of it belongs in a library repo. |
| Packaging | `Cargo.toml` uses an `exclude` list that doesn't cover `.codex/`, so `cargo package` would include these files. The CR-12 "final audit" dry run passed without catching this. |

**Actions:**

1. **Revoke and rotate** the tokens in `.codex/auth.json` now. Assume they're compromised for anyone who has had read access.
2. Purge the files from history (`git filter-repo --path .codex --invert-paths`) and force-push. Ask any collaborators to re-clone.
3. Add `.codex/`, `.roo/`, `.cursor/`, `.claude/`, `.specify/memory/` and `.firecrawl/` to `.gitignore`.
4. Replace `exclude = [...]` with an **allowlist**: `include = ["src/**", "migrations/**", "Cargo.toml", "README.md", "LICENSE*", "CHANGELOG.md"]`.
5. Add a CI step that runs `cargo package --list` and fails on any path outside the allowlist, plus `gitleaks` (or `trufflehog`) on every PR.

---

## 3. Correctness bugs

Severity: 🔴 data loss, hang or false guarantee · 🟠 incorrect under load · 🟡 latent.

### 3.1 Scheduling core

| # | Sev | Location | Defect | Fix |
|---|---|---|---|---|
| C1 | 🔴 | `core/worker_pool/native.rs:621` | **`max_units` is never enforced.** Each worker does `active_units.fetch_add(cost)` unconditionally after dequeuing, so the only limit is `worker_count`. Priority is ignored too: the crossbeam channel is FIFO. `PoolError::InsufficientCapacity` is never produced and `failed_tasks` is never incremented. The WASM pool sizes its semaphore by `worker_count`, not units. | Admission through a unit-weighted semaphore (§4.1), with a priority dispatcher in front of the workers. |
| C2 | 🔴 | `native.rs:635`, `resource_pool.rs:363` | **Executor panics aren't contained.** In `WorkerPool` the worker thread dies: its units are never released, `active_tasks` stays stuck, the result slot stays `Pending` (the caller waits out its full timeout), and the worker is never replaced. In `ResourcePool` the reserved units leak. | Wrap execution in `AssertUnwindSafe(fut).catch_unwind()`. Release capacity through an RAII guard (a semaphore permit). Store `Err(PoolError::ExecutorPanicked)`. Respawn dead workers or keep the loop alive. |
| C3 | 🔴 | `resource_pool.rs` submit/wake | **Lost wakeup.** `submit` fails the capacity check, then the last running task finishes and `try_wake` sees an empty queue, then `submit` enqueues. With nothing running, the task is parked forever. | After a successful enqueue, re-check capacity and trigger a wake if anything fits. Better: the semaphore and dispatcher design removes the race entirely. |
| C4 | 🔴 | `resource_pool.rs:315,335` and `try_wake_next_static` | **Silent task loss.** The depth check and the enqueue take the lock separately (TOCTOU). Separately, `try_wake` dequeues the head, finds it doesn't fit, and re-enqueues it. If a concurrent `submit` filled the freed slot in between, the re-enqueue fails and the task is **dropped** (only an error is logged). Re-enqueue also resets the task's FIFO position. | Use `peek` plus a conditional `pop` under one lock, and never pop-then-push. Do the depth check and the insert in one critical section. |
| C5 | 🔴 | `resource_pool.rs:685–775` | `sync_wake_worker_loop` is **publicly exported** and **reserves capacity, then discards the task** (`"caller would need to handle"`). It leaks units and loses work. It can't be enabled anyway, because `async_wake_enabled` has no setter. | Delete it, or finish it as an internal dispatcher. |
| C6 | 🟠 | `resource_pool.rs:242` (and two copies) | `current + cost` can **overflow `u32`** and wrap in release builds. A task with `cost > max_units` is **queued forever** instead of being rejected. | Use `checked_add`. Reject `cost > max_units` at submit with `PoolError::CostExceedsCapacity`. |
| C7 | 🟠 | `native.rs:613` | **Shutdown drops work.** A worker that sees the shutdown flag after `recv()` discards a task it already dequeued, and everything still buffered in the channel is dropped. Their result slots stay `Pending`, so `retrieve*` waits until timeout. | On shutdown, drain the channel, then mark each slot `Cancelled` and hand the task to the `CancellationSink` (CR-14's intended design). |
| C8 | 🟡 | `resource_pool.rs` | `prune_expired` deletes tasks without delivering `TaskStatus::Expired` to their mailbox, so clients never learn the task expired. Audit records a generic `"batch"` event. | Return the pruned tasks; deliver `Expired` and emit `LifecycleEvent::Expired` for each. |
| C9 | 🟡 | `core/executor.rs` | **Executors can't fail.** `execute` returns `T`/`R`, so `TaskStatus::Failed` is unreachable and errors have to be smuggled inside `T`. | `async fn execute(..) -> Result<T, Self::Error>`. |

### 3.2 Queue and mailbox backends

| # | Sev | Location | Defect | Fix |
|---|---|---|---|---|
| B1 | 🔴 | `infra/queue/postgres.rs:114,180,225,248`, `infra/mailbox/postgres.rs:179` | **`Handle::try_current()?.block_on(..)` panics inside a Tokio runtime** ("Cannot start a runtime from within a runtime"). `ResourcePool::submit` is async, and the integration tests are `#[tokio::test]`, so both hit it. These calls also run while holding a `parking_lot` mutex. | Make the `TaskQueue` and `Mailbox` traits async (§4.3). |
| B2 | 🔴 | `postgres.rs:212` | **At-most-once delivery.** The row is deleted inside the dequeue transaction, before the task runs, so a crash after dequeue loses the work. That contradicts the crate's core premise ("expensive task loss"). The schema has `locked_by`, `locked_at` and `attempts`, but nothing uses them. | Lease model: claim with `UPDATE … SET locked_by, locked_at, attempts = attempts + 1 … RETURNING`, `ack` deletes (or archives), `nack` clears the lease with a backoff. A reaper reclaims expired leases. |
| B3 | 🟠 | `postgres.rs:4,188,193` | The docs say `FOR UPDATE SKIP LOCKED`, but the code uses `.lock_exclusive()`, which emits plain `FOR UPDATE`. Concurrent dequeuers serialize behind the same head row. | Use `SKIP LOCKED` (SeaORM: `.lock_with_behavior(LockType::Update, LockBehavior::SkipLocked)`), or raw SQL (§7.4). |
| B4 | 🟠 | `postgres.rs:119,251` | `COUNT(*)` runs on **every enqueue and every `len()`**: O(n) per call, and the count-then-insert depth check races. | Keep an approximate depth (`pg_class.reltuples`, or a counter row updated in the same transaction), or enforce depth in one `INSERT … SELECT … WHERE (SELECT count…) < $max`. |
| B5 | 🔴 | `infra/mailbox/postgres.rs` | **The Postgres mailbox is write-only.** `consume` and `evict_expired` aren't implemented, so the trait defaults return `None` and `0`. `task_id` is never set and there's no idempotency handling. | Implement `consume` as `DELETE … RETURNING`, TTL eviction, and first-write-wins on `idempotency_key` (`INSERT … ON CONFLICT DO NOTHING`). |
| B6 | 🔴 | `infra/queue/yaque.rs:82–117` | **Not crash-safe.** Every dequeue rewrites the whole JSONL file with `truncate(true)`: no temp-file-plus-rename and no `fsync`, so a crash mid-write wipes the queue. O(n) I/O per dequeue. **FIFO only**, which contradicts "priority-based dequeuing". It isn't the `yaque` crate. | Replace it with a redb- or fjall-backed queue, or adopt `rustqueue` (§9). |
| B7 | 🟠 | `infra/queue/memory.rs:26–54` | `Eq` compares `id` but `Ord` compares priority and `created_at_ms`, which breaks the `Eq`/`Ord` contract. The FIFO tiebreak uses the caller-supplied `created_at_ms`, so tasks submitted in the same millisecond come out in arbitrary order. | Add a monotonic `seq: u64`, assigned at enqueue, and order by `(priority, Reverse(seq))`. Derive `Eq` from the same key. |
| B8 | 🟡 | `migrations/003_install_pgmq.sql` | `COMMENT ON EXTENSION pgmq` sits *outside* the guarded `DO` block, so the migration **fails** whenever pgmq isn't installed, which defeats the guard. | Move the comment inside the `DO` block, or drop it. |
| B9 | 🟡 | `core/audit.rs:89` | `PostgresAuditSink::record` is a **no-op stub**, but it's publicly exported as if it worked. | Remove it, or implement it as a `Hook` (better: publish to Iggy, §8). |

### 3.3 Config and builders

- **The documented JSON config doesn't deserialize.** `QueueBackendConfig` is a unit enum (`InMemory | File | Postgres`) with no `queue_name`, `path`, `stream` or `notifier` fields. The JSON in README, CLAUDE.md and `docs/` (`"type": "postgres_pgmq"`, `"path"`, `"notifier"`) doesn't match it.
- **`RuntimeConfig::{WebWorker, CloudWorker}` are inert.**
- **`build_pools_from_scheduler_config` and `HttpCallbackNotifier` are documented but don't exist.**

---

## 4. Patterns that are missing or only half-built

### 4.1 Unit-weighted semaphore plus priority dispatcher (replaces the CAS loop, condvar and wake code)

```text
submit ──► PriorityQueue (in-mem index; durable backend underneath)
                │
        Dispatcher task (single writer)
           loop {
             wait for: new task OR capacity released
             pick = head if fits else first-that-fits (skip-ahead),
                    unless head.starvation_score > threshold → reserve for head
             permit = sem.try_acquire_many_owned(pick.cost)?   // OwnedSemaphorePermit
             spawn(execute(pick), permit)                       // permit drops on completion/panic
           }
```

- The capacity check, reservation and release become one primitive, `tokio::sync::Semaphore` with `max_units` permits. **The permit is released on drop, including during a panic unwind.** That fixes C2, C3, C4 and C6 together.
- A **single dispatcher** gives the "single-writer unpark" and **skip-ahead bin-packing with a starvation guard** that `docs/PARKING_LOT.md` specifies but nothing implements.
- **Vectorized resources** (VRAM *and* worker slots, per `PARKING_LOT.md` §5): one semaphore per `ResourceKind`, acquired in a fixed order, or a small `ResourceVector` ledger under the dispatcher's lock.
- On native, `WorkerPool` keeps its dedicated OS threads, but receives work from the dispatcher instead of directly from submitters.

### 4.2 One shared inner struct

`ResourcePool` clones 10 `Arc`s per task and threads them through `#[allow(too_many_arguments)]` static functions that return `Pin<Box<dyn Future>>`. Put the shared state in one `Arc<Inner>`. That removes about 200 lines, the duplicated spawn code and the boxed futures.

### 4.3 Async queue trait with lease and ack

```rust
#[async_trait]
pub trait TaskQueue<P>: Send + Sync {
    async fn enqueue(&self, task: ScheduledTask<P>) -> Result<(), PoolError>;
    async fn lease(&self, max: usize, lease_for: Duration) -> Result<Vec<Leased<P>>, PoolError>;
    async fn ack(&self, lease: LeaseId) -> Result<(), PoolError>;
    async fn nack(&self, lease: LeaseId, retry_after: Option<Duration>) -> Result<(), PoolError>;
    async fn extend(&self, lease: LeaseId, by: Duration) -> Result<(), PoolError>; // heartbeat
    async fn prune_expired(&self, now: SystemTime) -> Result<Vec<ScheduledTask<P>>, PoolError>;
    async fn approx_len(&self) -> Result<usize, PoolError>;
}
```

- This gives **at-least-once** delivery, which the locked decisions D2, D3 and D7 (`decision-idempotency.md`) already assume.
- Long LLM jobs get **heartbeat extension**, so they aren't redelivered mid-run.
- `nack` with backoff becomes the replay path CR-14 promised.
- The in-memory backend implements the trait trivially; Postgres, pgmq and embedded stores map onto it directly.

### 4.4 Result-typed execution and results

- `WorkerExecutor::execute(..) -> Result<R, E>`, wrapped in `catch_unwind`.
- Result slots hold `Result<R, PoolError>`.
- `TaskStatus::Failed` and `failed_tasks` become reachable.

### 4.5 Wire the subsystems that already exist, or remove them

- **`HookBus`:** emit `Submitted`, `Queued`, `Started`, `Completed`, `Failed`, `Rejected`, `Expired` and `Cancelled` from the dispatcher. Register hooks on the builder.
- **`ShutdownHandle::shutdown_with(_policy)`** ignores its policy. Implement `Drain`, `DrainThenCancel` and `CancelImmediately` in the dispatcher.
- **`CancellationChannel` / `CancellationSink`:** feed them from shutdown and deadline cancellation, and connect `SinkOutcome::Requeue` to `nack`.
- **`CancellationChannel::new`** calls `tokio::spawn` unconditionally, so it panics outside a runtime. Take a `Spawn` impl instead.

### 4.6 Result-slot lifetime

Results that are never retrieved stay in `ResultStorage` forever, including results holding streaming channels. Add a per-slot TTL (default to the task's `default_timeout`) and a sweeper.

### 4.7 Clock abstraction

`IMPLEMENTATION.md` Phase 0 specified a `Clock` trait. What exists is a free `now_ms() -> u128` function. Introduce `trait Clock { fn now(&self) -> SystemTime; fn instant(&self) -> Instant; }` so deadline and TTL logic is testable without sleeping, and use `u64` milliseconds throughout.

---

## 5. Missed optimizations

| Area | Issue | Optimization |
|---|---|---|
| `WorkerPool::retrieve_async` (`native.rs:419`) | Each waiting caller occupies a **`spawn_blocking` thread** parked on a condvar. With 120 s LLM timeouts, a few hundred concurrent waiters exhaust Tokio's blocking pool (512 threads by default) and stall every other `spawn_blocking` user, file I/O included. CR-01 bounded the wait but kept the design. | Give each slot a `tokio::sync::oneshot` (or `Notify`). Async waiters cost nothing; the blocking API uses `blocking_recv`. |
| Result map | Every operation allocates a `format!("worker_pool:{id}")` key, hashes a `String`, and takes one global `RwLock`. | Key by `u64`. Use a sharded map (`DashMap`, or `papaya`) or a `slab` indexed by task id. |
| Postgres dequeue | 4 round trips: BEGIN, SELECT … FOR UPDATE, DELETE, COMMIT, plus `Value` clones of JSONB. | One statement (§7.4). Deserialize straight from `&serde_json::Value` without cloning, or store payloads as `bytea` (MessagePack or bincode) when they don't need to be queryable. |
| Postgres depth | `COUNT(*)` on every enqueue. | Approximate count, or a counter row (B4). |
| Wake path | Every completion locks the condvar mutex and calls `notify_one`, even in async-wake mode, then spawns a new wake task per completion. | Use one dispatcher task woken by `Notify` (§4.1). |
| Logging | `tracing::info!` fires per task submit, start and complete. | Lower to `debug!`/`trace!`, and use spans with fields instead of formatted strings. |
| Audit | `Arc<Mutex<Box<dyn AuditSink>>>`: double indirection plus a global lock on the hot path. | Make it a `Hook`, and move I/O off the hot path through a bounded channel. |
| In-memory prune | `drain()`, `collect`, rebuild. | `BinaryHeap::retain` (stable since 1.70). |
| Yaque enqueue | Requires `P: Clone` and clones every payload. | Serialize once, append, then push the owned value. |
| Timestamps | `u128` milliseconds, cast to `i64` for Postgres. | `u64`, or `SystemTime` at the API surface. |
| Dependencies | Unused: `dotenvy`, `lock_api`, `parking_lot_core`, `uuid`, direct `sqlx`, and `pgmq` (declared but no code uses it). `num_cpus` can be replaced with `std::thread::available_parallelism`. A library shouldn't force `tracing-subscriber` or `anyhow` on its users. | Prune them, and feature-gate `telemetry`. Smaller compile graph, fewer advisories. |

---

## 6. Build, tests and repository hygiene

- **Two whole test suites are never compiled.** `tests/unit/*` and `tests/integration/*` are subdirectories, and no top-level `tests/*.rs` file declares them as modules. Cargo only builds top-level files in `tests/`, so the unit and Postgres integration suites have **never run**.
- **They wouldn't compile anyway.** They use `testcontainers::clients::Cli` and `RunnableImage`, APIs removed long before the 0.23 version the crate depends on.
- **The coverage baseline is therefore overstated.**
- **WASM is never built.** `core/worker_pool/wasm.rs` uses `tokio::spawn`, which isn't available on `wasm32-unknown-unknown`, and CI has no wasm job. Either add `cargo check --target wasm32-unknown-unknown --no-default-features` with `wasm-bindgen-futures::spawn_local`, or drop the claim.
- **`#![deny(warnings)]` in `lib.rs`** combined with clippy `pedantic` + `nursery` means each new stable toolchain can break the build. Leave enforcement to CI's `-D warnings`.
- **The MSRV is pinned in `rust-toolchain.toml` (1.91.1).** Contributors without that toolchain installed can't build offline. Keep the MSRV in `rust-version` and the CI matrix, and let `rust-toolchain.toml` track `stable`.
- **`WorkerPool::new` panics** via `.expect("Failed to spawn worker thread")`. Return `PoolError::Internal` instead.
- **Crate metadata is wrong.** `repository` and `homepage` point to `prometheus-ai/prometheus-parking-lot`, which isn't this repo. CR-08 is marked done.
- **Doc drift.**
  - `CLAUDE.md` and the README show APIs that don't exist (`build_pools_from_scheduler_config`, `HttpCallbackNotifier`, `TaskId(1)`, `TenantId`), old dependency versions (thiserror 1, sea-orm 0.12) and features that don't exist (`yaque`, `config`, `native`). Coding agents working from `CLAUDE.md` will produce non-compiling code.
  - Rewrite `CLAUDE.md` from the actual public API.
- **`.kbd-orchestrator` contradicts itself.**
  - `progress.json` lists 14/14 changes done.
  - Its own `milestone_status` says "M2-harden: not started" and "CR-04 pending".
  - `current-waypoint.md` still says "Plan complete → next is Execute".

---

## 7. Postgres 18 compatibility

PostgreSQL 18 shipped in September 2025. This section covers what the crate needs, given that the test infrastructure currently pins `postgres:17-alpine` (`docker-compose.test.yml`, `tests/integration/mod.rs:54`).

### 7.1 Compatibility checklist

| Area | PG18 change | Impact on this crate | Action |
|---|---|---|---|
| SQL in migrations | No relevant removals. `BIGSERIAL`, `JSONB`, `TIMESTAMPTZ`, partial indexes and `CREATE … IF NOT EXISTS` are unchanged. | **Compatible** as far as I can tell from reading (not executed). | Run the migrations against PG18 in CI. |
| Wire protocol | PG18 adds protocol 3.2 (256-bit cancel keys) and libpq min/max protocol settings. 3.0 clients are still accepted. | sqlx 0.8 / SeaORM 1.1 speak 3.0 and **keep working**. 3.2 is optional. | None required. Track sqlx support for 3.2. |
| Authentication | **MD5 passwords deprecated**: `CREATE/ALTER ROLE` with MD5 now warns, and removal is planned. | Docker images default to SCRAM. The concern is user-managed servers still on `md5` in `pg_hba.conf`. | Document SCRAM as required. Add a startup check that warns if the connection authenticated via MD5. |
| Data checksums | `initdb` now enables checksums by default. `pg_upgrade` needs the old and new clusters to match. | Operational only: doesn't affect the crate's code. | Add to the upgrade runbook: `--no-data-checksums` in `POSTGRES_INITDB_ARGS`, or `pg_checksums --enable` on the old cluster first. |
| Docker image layout | `postgres:18` expects data under `/var/lib/postgresql/18/docker` and wants the **parent** directory `/var/lib/postgresql` mounted. Mounting the old `/data` path silently initializes an **empty** cluster. | The test compose file mounts only `docker-entrypoint-initdb.d`, so it's unaffected. **Any dev or prod compose files with persistent volumes are affected.** | Update volume mounts in every compose file and Helm chart. Add a warning to the README. |
| pgmq extension | pgmq added **PG18 support in v1.8.0**. The current release is v1.13 (FIFO groups, topic routing, insert notifications, partition fixes). | The crate depends on the Rust `pgmq` crate at a **pre-release `0.34.0-alpha.5`** but uses none of it. The alpine image has no pgmq. The migration is buggy (B8). | Either drop the `pgmq` feature, or implement it against pgmq ≥ 1.8 using an image that bundles pgmq, or install pgmq's SQL-only build. |
| Vacuum on queue tables | PG18 adds `autovacuum_vacuum_max_threshold`, eager freezing, asynchronous I/O for vacuum, and vacuum timing in `pg_stat_all_tables`. | Queue tables churn heavily (insert, then delete). | Set per-table autovacuum: `ALTER TABLE pl_queue_jobs SET (autovacuum_vacuum_scale_factor = 0, autovacuum_vacuum_threshold = 1000)`. On PG18, consider `autovacuum_vacuum_max_threshold`. Monitor with `pg_stat_io`. |
| ORM | SeaORM 2.0 shipped (July 2026). The 1.x API still works and the main breaking area is the new entity format. | The crate is on SeaORM 1.1. | Decide whether to keep SeaORM at all. The queue and mailbox hot paths are better as raw `sqlx` statements (§7.4). Keep SeaORM only if entity generation earns its place. |

### 7.2 PG18 features worth using (optional; keep PG17 working)

- **`uuidv7()`** for job and mailbox IDs: time-ordered keys index better than v4. Keep `BIGSERIAL` as the default and make UUIDv7 opt-in, or generate v7 client-side (`uuid` crate `v7` feature) so PG17 still works.
- **`RETURNING old.*, new.*`** on the lease `UPDATE`: one round trip returns both the prior lease state and the new one, which helps the reaper and the audit trail.
- **B-tree skip scan:** lets the existing `(queue_name, priority DESC, created_at)` index serve cross-queue admin queries that don't filter on `queue_name`.
- **Virtual generated columns** (now the default): e.g. `is_leased boolean GENERATED ALWAYS AS (locked_at IS NOT NULL) VIRTUAL` for monitoring, at no write cost.
- **`log_lock_failures`:** useful during lease contention debugging. It covers `NOWAIT`, not `SKIP LOCKED`.

### 7.3 CI changes

- Test matrix: `postgres:17` and `postgres:18`, plus a pgmq-bundled PG18 image if the pgmq feature stays.
- A real service-container job (GitHub Actions `services:`) that runs the integration suite once it's fixed (§6).

### 7.4 Reference lease SQL (PG 17 and 18 compatible)

```sql
-- claim N jobs
WITH next AS (
  SELECT id FROM pl_queue_jobs
  WHERE queue_name = $1
    AND (locked_at IS NULL OR locked_at < now() - $2::interval)   -- expired lease reclaim
    AND (deadline_ms IS NULL OR deadline_ms > $3)
    AND (not_before IS NULL OR not_before <= now())
  ORDER BY priority DESC, id ASC
  LIMIT $4
  FOR UPDATE SKIP LOCKED
)
UPDATE pl_queue_jobs j
SET locked_by = $5, locked_at = now(), attempts = attempts + 1
FROM next WHERE j.id = next.id
RETURNING j.id, j.metadata, j.payload, j.attempts;

-- ack
DELETE FROM pl_queue_jobs WHERE id = $1 AND locked_by = $2;

-- nack with backoff
UPDATE pl_queue_jobs SET locked_by = NULL, locked_at = NULL,
       not_before = now() + $3::interval
WHERE id = $1 AND locked_by = $2;

-- supporting index
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_claim
  ON pl_queue_jobs (queue_name, priority DESC, id)
  WHERE locked_at IS NULL;
```

This needs a `not_before TIMESTAMPTZ` column. Add `LISTEN/NOTIFY` (a `pg_notify('pl_queue_'||queue_name, '')` trigger) so idle dispatchers wake without polling.

---

## 8. Apache Iggy: event propagation and mailbox

### 8.1 Where Iggy is today (as of 0.9.0, September 2026)

- **Architecture:** a thread-per-core, shared-nothing server on `io_uring`. 0.9.0 made it the only server binary.
- **Clustering:** VSR-consensus clustering now ships in the standard binary. Single-node is still the default; three replicas tolerate one failure, five tolerate two.
- **Rust SDK 0.11:** node failover, consumer-group rejoin after membership loss, and deferred long-polling (bounded wait, minimum message count, byte cap).
- **Delivery model:**
  - Retried produces are **deduplicated per partition**.
  - Consumers track progress with **offsets**, auto-committed or stored explicitly. There is **no per-message ack and no key-based lookup**.
  - Retention expires whole **segments**, not individual messages.
- **Maturity:** still **pre-1.0**. 0.9.0 changed the on-disk format with **no migration from 0.8.x**, and changed the wire protocol, so the server and SDKs must be upgraded together.

### 8.2 Fit assessment

| Use | Fit | Reasoning |
|---|---|---|
| **Lifecycle event propagation** (`HookBus` → other services, dashboards, audit) | ✅ **Strong** | An append-only, replayable, partitioned log is the right shape. It replaces the no-op `PostgresAuditSink`, gives cross-node visibility, and lets late subscribers replay from an offset or timestamp. It also matches the agent-mesh design, where Iggy is already the message layer. |
| **Mailbox notification** (tell a gateway or AG-UI bridge that a result is ready) | ✅ **Strong** | It replaces the never-built `HttpCallbackNotifier`. A reconnecting client resumes from its stored offset, so no webhook retry logic is needed. |
| **Streaming-token replay for disconnected clients** | ✅ **Good** | Publish each LLM token chunk to a per-session partition. A reconnecting client replays from where it left off. Today the non-serializable `flume::Receiver` result is simply lost if the client disconnects. |
| **Mailbox storage** (keyed get, consume-once, per-message TTL, first-write-wins) | ❌ **Poor** | No key lookup, no per-message delete, segment-level expiry. Store results in Postgres, redb or an object store, and **publish a pointer event** to Iggy. |
| **The parking-lot work queue itself** | ❌ **Poor** | No priorities (only approximated by topics per priority), no per-message ack or visibility timeout, and offset commits per partition cause head-of-line blocking. Capacity-aware skip-ahead isn't expressible. |
| **Distributed ingestion buffer** (many front ends → one scheduler per partition) | ⚠️ **Possible** | Front ends produce submissions to Iggy. Each scheduler instance consumes its partitions and moves tasks into its leased queue. This adds a hop; only worth it when submit volume or fan-in really needs it. |

### 8.3 Recommended integration

Put it behind a feature flag (`iggy`), and keep it out of `core`:

```text
crate feature "iggy"
 ├── IggyHook: Hook                 → stream "parking-lot", topic "lifecycle.{pool}",
 │                                    partition key = tenant (ordering per tenant)
 ├── IggyNotifier: MailboxNotifier  → topic "mailbox.ready", payload = {mailbox_key, task_id,
 │                                    status, result_ref}, message id = UUIDv5(idempotency_key)
 └── IggyStreamSink (optional)      → topic "stream.{session}" for token replay
```

- **Message ID:** derive the 128-bit client message ID from `idempotency_key` (UUIDv5) so retried publishes deduplicate.
- **Emit from a bounded channel drained by a background task**, never inline on the dispatcher. Keep `HookBus` synchronous and cheap.
- **Pin `iggy` exactly** (e.g. `=0.11.x`) and pin the server version in compose. Upgrading crosses an on-disk and wire break until Iggy reaches 1.0.
- **Mailbox storage** becomes `Mailbox = Storage (PG/redb) + Notifier (Iggy | None)`, the composition `IMPLEMENTATION.md` Phase 4 described but nobody built.

---

## 9. Rust queue options that have appeared since this crate was built

### 9.1 Landscape

| Option | Storage | What it offers | Relevance |
|---|---|---|---|
| **apalis 1.0 (rc) + `apalis-postgres`** (1.0.0-rc.2, Jan 2026) | Postgres | Priority, heartbeats with orphaned-job re-enqueue, LISTEN/NOTIFY, trigger-based storage. Sibling `apalis-pgmq` adapter. | A full worker framework. Strong reference for lease and heartbeat design. Adopting it would mean handing worker execution to apalis, which conflicts with the dedicated-OS-thread GPU worker model. |
| **underway 0.2** (Oct 2026) | Postgres (sqlx ≥ 0.8.2) | Durable multi-step jobs (resume at the failed step), retries with strategies, cron schedules, **enqueue inside your own transaction**, `FOR UPDATE SKIP LOCKED`. | Good fit for multi-step agent workflows layered **above** the parking lot. Priority isn't mentioned. |
| **graphile_worker_rs 0.13** | Postgres | Priority, job keys, named queues, LISTEN/NOTIFY, cron, schema compatible with Node Graphile Worker. | Mature, but brings its own schema and executor model. |
| **pgmq 1.13** (extension) | Postgres | SQS-style visibility timeouts (leases), archive tables (dead-letter queue), FIFO groups, topic routing, insert notifications, PG18 support since 1.8. | Best fit for an **optional cloud backend** behind the new lease trait. No native priority: use one queue per priority level. The Rust client crate lags (the crate pins an alpha). |
| **rustqueue 0.3** (Oct 2026) | redb (embedded ACID) | Embedded push/pull/ack, backoff strategies, can run later as a standalone server (`rustqueue serve`). | **Candidate replacement for the JSONL Yaque queue** on desktop/Tauri. Pre-1.0 with thin docs: evaluate or vendor. |
| **agent-queue / tauri-queue 0.3** | Embedded (backend not documented) | Job lifecycle events bridged to the Tauri frontend, priorities, coalescing emitters with backpressure. | Useful reference for Tauri UX (progress events), not as the core queue. |
| **effectum 0.7** | SQLite | Priority, retries with backoff, checkpoints, crash recovery, recurring jobs. | Good design, but the last release was July 2024. Treat it as stale. |
| **redb / fjall** (storage engines) | Embedded B-tree / LSM | ACID transactions and atomic batches. fjall has keyspaces. | **Build the embedded queue directly on one of these**, with key = `(priority_inv, seq)`, which gives priority order for free. |
| **NATS JetStream** (`async-nats`) | Server | Work-queue retention, per-message ack/nak with redelivery, max-deliver limits, and a key-value store. | Closer to queue semantics than Iggy. Worth considering only if you'd run NATS anyway. Given the Iggy commitment in the agent mesh, not recommended as an extra broker. |
| **Apache Iggy** | Server | §8. | Events and notifications, not the work queue. |

**Note:** `postmodern` (a Postgres-backed job queue) also turned up; I couldn't retrieve its details, so it's unverified.

### 9.2 Recommendation

**Don't replace the scheduler with any of these.** The crate's differentiator is **unit-weighted, multi-resource admission with priority and skip-ahead**. None of these frameworks provides that; they limit concurrency per worker, not weighted capacity. Instead:

1. Own the **lease/ack trait** (§4.3) and the dispatcher (§4.1).
2. Ship backends in this order:
   - (a) in-memory;
   - (b) a raw-sqlx Postgres lease queue (§7.4), the default for cloud;
   - (c) a **redb-backed embedded queue**, the default for Tauri, replacing JSONL, or `rustqueue` after evaluation;
   - (d) optionally, pgmq ≥ 1.8.
3. Use **underway** (or apalis) *above* the parking lot for multi-step durable agent workflows, where each step that needs GPU capacity submits to the parking lot.
4. Use **Iggy** for events and notifications (§8).

---

## 10. Review of the plans and phases in `docs/` (and `.kbd-orchestrator/`)

### 10.1 Document-by-document verdict

| Document | Verdict | Notes |
|---|---|---|
| `WHAT_WHY.md` | ✅ **Valid** | An accurate statement of intent. Keep it as the north star. |
| `PARKING_LOT.md` | ✅ **Valid, not implemented** | Skip-ahead bin-packing with a starvation guard, single-writer unpark, OCC/lease claim, vectorized resource costs. **None of it is implemented.** Carry into the next phase (§11). |
| `IMPLEMENTATION.md` (Phases 0–6) | ⚠️ **Partially valid / stale** | Phase 0: no `Clock` trait. Phase 1: done but buggy (§3). Phase 2: done but at-most-once and panics (B1/B2). Phase 3: **pgmq not done**, and "Yaque" doesn't use the yaque crate. Phase 4: **HTTP notifier not done**; the storage/notifier split doesn't exist. Phase 5: only a factory-closure `build_pools`; config enums carry no backend parameters. Phase 6: two examples exist, `local_tauri.rs` and `cloud_server.rs` don't. Replace with §11. |
| `ARCHITECTURE.md` | ⚠️ **Partially valid** | Context and scenarios are fine. §7 performance tables come from old micro-benchmarks and describe the CAS and condvar design that §4.1 replaces. "Persistent queues survive restarts" is currently false (B2, B6). Update after the rework. |
| `PARKING_LOT_FINAL_MIGRATION.md` | ⚠️ **Valid as API docs, wrong on guarantees** | Accurately documents the `WorkerPool` API, but implies capacity enforcement (C1). It will need rewriting when executors return `Result`. |
| `PUBLISHING.md` | ✅ **Valid** | Add an allowlist/`cargo package --list` gate and a secret scan to the pre-flight checks (§2). |
| `coding-standards/README.md` | ✅ **Valid** | Microsoft Pragmatic Rust Guidelines. Keep it. |
| `README.md` (docs index) | ⚠️ **Stale** | The sample config uses string backends that match the code, but other docs contradict it. Make this the single source of truth. |
| `SPEC-`, `PLAN-`, `TASKS-MUTEX` / `RWLOCK` / `ONCE` / `CONDVAR` (11 files) | ❌ **Invalid** | They describe a **different crate**: a re-export wrapper around `parking_lot` with `src/mutex.rs`, `src/rwlock.rs`, `src/once.rs` and `src/condvar.rs`, none of which exist here. Archive them. |
| `TODO.md` | ❌ **Invalid** | Same primitives project ("Phase 2: Condvar awaiting verification", 2024-12-06). Replace it. |
| `API.md` | ❌ **Invalid** | Documents the `Mutex<T>`/`RwLock<T>`/`Condvar` API. Replace it with generated rustdoc and a short API overview. |
| `CHANGELOG.md` | ❌ **Invalid** | Its "Added" entries are Mutex, RwLock, Once and Condvar. Reset it to the real history (worker pool, certification remediation, Postgres backend). |
| `Comprehensive Architectural Report …` | ❌ **Mostly not applicable** | Physical parking-garage OOD (vehicles, spots, surge pricing) plus a Redis audit pattern. §4.1.2 (OCC) and the tracing/audit-layer ideas are the only relevant parts. Already excluded from packaging; move to `docs/archive/`. |
| `DISCUSSION.md` | 🗄️ **Historical** | The original prompt chain (ReddDb, "Option C", early layouts). Superseded. Archive it. |
| `.kbd-orchestrator/phases/phase-certification-remediation/*` | ⚠️ **Marked complete, partly unfinished** | See §10.2. |
| `specs/001-parking-system-build/*` | 🗄️ **Superseded** | No checkbox progress tracked (0 checked, 0 unchecked). Archive, or fold into the new phase spec. |

### 10.2 Certification phase: claimed vs. actual

| CR | Claimed | Actual |
|---|---|---|
| CR-01 `retrieve_async` leak | Done | **Partly done.** The wait is now bounded and slot removal wakes the waiter, but it still takes one blocking-pool thread per waiter (§5). |
| CR-02 shutdown join timeout | Done | Done (detaches wedged workers). In-flight and queued work is still dropped (C7). |
| CR-03 Ctrl-C/SIGTERM | Done | Signal helper exists. **Not connected to any pool.** |
| CR-04 coverage + watchdog | Done | `progress.json` itself says "pending". Two suites never compile (§6), so the coverage numbers are overstated. |
| CR-05 error taxonomy | Done | `SchedulerError` exists, but `PoolError` is a separate hand-written enum, so there are two taxonomies. |
| CR-06 mailbox hardening | Done | In-memory mailbox done. **The Postgres mailbox has no `consume` or eviction** (B5). |
| CR-07 hooks | Done | Types only. **None of the six tasks in its own change file is checked off.** No emission points. |
| CR-08 metadata + license | Done | License done. **The repository and homepage URLs are still wrong.** |
| CR-09 docs pass | Done | CLAUDE.md, README and docs drift (§6, §10.1). |
| CR-10 CI gates | Done | CI exists. No wasm, no Postgres service, no secret scan, no semver check. |
| CR-12 final audit | Done | Missed the committed credentials and the dead test suites. |
| CR-13 pluggable shutdown | Done | `shutdown_with` **ignores the policy**. |
| CR-14 cancellation capture/replay | Done | Types and a drain task exist. **No pool produces `CancelledTask`, and there's no nack/redelivery** (no backend supports it). |

**Takeaway:** the phase-complete flag should be reverted to "partially complete". The unfinished items roll into the next phase.

---

## 11. Recommended next phase

### Phase: `phase-correctness-and-durability` (v0.2.0)

**Goal:** make the crate's three promises true: weighted capacity is never exceeded, accepted work is never silently lost, and results reach disconnected clients. Support PG 17 and 18. Integrate Iggy for events.

#### Milestone 0: Containment (do first, about 1 day)

| ID | Change | Acceptance |
|---|---|---|
| CR-15 | Revoke the leaked tokens, purge history, `.gitignore`, `include` allowlist, gitleaks in CI | `cargo package --list` shows only allowlisted paths. The secret scan is green. |
| CR-16 | Archive the invalid docs (§10.1) to `docs/archive/`. Reset CHANGELOG, TODO and API. Rewrite CLAUDE.md from the real API. Fix the repository and homepage URLs. | Every code sample in CLAUDE.md and the README compiles as a doctest. |
| CR-17 | Correct `.kbd-orchestrator` progress for the certification phase | `progress.json` matches §10.2. |

#### Milestone 1: Core rework (the critical path)

| ID | Change | Fixes | Acceptance |
|---|---|---|---|
| CR-18 | `Arc<Inner>` refactor of `ResourcePool`; delete `sync_wake_worker_loop` | §4.2, C5 | No `too_many_arguments`, no boxed static futures. |
| CR-19 | **Dispatcher plus unit-weighted semaphore**, shared by `ResourcePool` and `WorkerPool` (native and WASM) | C1, C3, C4, C6 | A loom or shuttle model test proves `Σ active cost ≤ max_units` and no lost wakeup. A property test runs random submit/complete interleavings. |
| CR-20 | Skip-ahead bin-packing with a starvation guard (configurable), per `PARKING_LOT.md` §3.3 | Design gap | A large head task runs within N dispatch cycles. Utilization benchmark beats strict FIFO. |
| CR-21 | `Result`-returning executors, `catch_unwind`, RAII permits, worker respawn | C2, C9 | A panicking executor doesn't leak units or kill the pool. `failed_tasks` increments. |
| CR-22 | `oneshot`-based result slots with TTL sweeper; `u64` keys | §5, §4.6 | 10k concurrent `retrieve_async` calls use 0 blocking threads. |
| CR-23 | Multi-resource cost vector (VRAM plus worker slots) | `PARKING_LOT.md` §5 | A task needing (8 GB, 1 slot) waits when either resource is short. |
| CR-24 | Monotonic sequence for FIFO; `Eq`/`Ord` consistency | B7 | Deterministic ordering test. |

#### Milestone 2: Durable backends

| ID | Change | Acceptance |
|---|---|---|
| CR-25 | **Async `TaskQueue` with lease, ack, nack and extend** (§4.3); async `Mailbox` | No `block_on` anywhere in `src/`. |
| CR-26 | Postgres lease queue with raw sqlx (§7.4), NOTIFY wake-up, reaper, `not_before` backoff | A kill -9 mid-task leads to redelivery after the lease expires. N dispatchers never double-claim (SKIP LOCKED). |
| CR-27 | Postgres mailbox: `consume` as `DELETE … RETURNING`, TTL eviction, idempotency `ON CONFLICT` | Consume-once and first-write-wins tests against real PG. |
| CR-28 | Embedded queue on **redb** (or `rustqueue` after a one-day spike) replacing JSONL "Yaque" | A crash at any point during dequeue loses nothing (fault-injection test). Priority order is honored. |
| CR-29 | Wire `ShutdownPolicy`, the cancellation channel and `SinkOutcome::Requeue` → `nack` | `DrainThenCancel` captures stragglers. The requeued task runs after restart (PG and redb). |
| CR-30 | Decide on pgmq: implement against pgmq ≥ 1.8 with one queue per priority, **or** remove the feature and dependency | No unused feature flags remain. |

#### Milestone 3: Events and notification (Iggy)

| ID | Change | Acceptance |
|---|---|---|
| CR-31 | Wire `HookBus` emission at every lifecycle transition; register hooks on the builder | Event-ordering test. Zero overhead with no hooks (benchmark). |
| CR-32 | `iggy` feature: `IggyHook` and `IggyNotifier`; UUIDv5 message IDs from the idempotency key; bounded off-path publisher | An integration test against a pinned Iggy server container shows a subscriber receiving `mailbox.ready` and replaying from an offset. |
| CR-33 | (Stretch) `IggyStreamSink` for streaming-token replay to reconnecting clients | A client disconnects mid-stream, reconnects and receives the remaining tokens with no gap or duplicate. |
| CR-34 | Mailbox composition: `StorageMailbox<S> + Option<Notifier>` | Notifier failure doesn't lose the stored result (test). |

#### Milestone 4: Platform and CI

| ID | Change | Acceptance |
|---|---|---|
| CR-35 | Make `tests/unit` and `tests/integration` real test targets; migrate to the current testcontainers API | Both suites run in CI. |
| CR-36 | Postgres CI matrix **17 + 18** (plus a pgmq-PG18 image if CR-30 keeps pgmq); per-table autovacuum settings in migrations; PG18 runbook (checksums, Docker volume path, SCRAM) | Green on both versions. |
| CR-37 | Config schema that carries backend parameters (`queue_name`, `path`, `lease_secs`, `notifier`); a real `build_pools_from_scheduler_config` | Every JSON example in the docs deserializes (test). |
| CR-38 | WASM: `cargo check --target wasm32-unknown-unknown` job with `spawn_local`, or drop the WASM claim | Decision recorded; CI enforces it. |
| CR-39 | Dependency prune (§5); remove `#![deny(warnings)]`; `rust-toolchain.toml` → stable; `cargo-semver-checks` | `cargo udeps` clean. |
| CR-40 | Re-run benchmarks; rewrite `ARCHITECTURE.md` §7 with new numbers | Published numbers come from CI-reproducible benchmarks. |

**Dependency order:** M0 → M1 (CR-18 → CR-19 → CR-20/21/22/23) → M2 (CR-25 → CR-26/27/28 → CR-29) → M3 (CR-31 → CR-32/34 → CR-33). M4 runs in parallel after CR-25.

**Out of scope for this phase:** multi-node scheduler coordination (one dispatcher per partition via Iggy ingestion or PG advisory locks). That should be a later phase, once single-node semantics are proven.

---

## 12. Sources

- PostgreSQL 18 release notes (mirror): https://dev.begriffs.com/ai-pg/blob/HEAD:/full-docs/txt/release-18.txt
- PG18 data checksums by default (credativ): https://www.credativ.de/en/blog/postgresql-en/postgresql-18-enables-datachecksums-by-default/
- PG18 Docker image data-path change: https://rezhajul.io/posts/upgrading-postgresql-18-docker/
- pgmq releases (PG18 support in 1.8.0, latest 1.13): https://github.com/pgmq/pgmq/releases
- pgmq Rust crate: https://lib.rs/crates/pgmq
- Apache Iggy 0.9.0 release: https://iggy.apache.org/blogs/2026/09/21/release-0.9.0/
- Apache Iggy concepts: https://iggy.incubator.apache.org/docs/introduction/concepts/
- SeaORM 2.0 announcement: https://www.sea-ql.org/blog/2026-07-27-sea-orm-2.0/
- apalis-postgres: https://lib.rs/crates/apalis-postgres
- underway: https://docs.rs/underway
- graphile_worker_rs: https://github.com/leo91000/graphile_worker_rs
- rustqueue: https://docs.rs/rustqueue
- tauri-queue: https://docs.rs/tauri-queue
- effectum: https://lib.rs/crates/effectum
