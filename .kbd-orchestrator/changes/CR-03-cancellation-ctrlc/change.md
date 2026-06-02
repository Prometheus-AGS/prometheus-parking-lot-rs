# CR-03 — Cancellation token + Ctrl-C/SIGTERM graceful shutdown

- **Track:** A/B · **Severity:** 🟠 High · **Depends on:** CR-02
- **Skill:** `rust-async-patterns` · **Agent:** Plan → general-purpose
- **Files:** `src/core/worker_pool/native.rs`, `src/core/resource_pool.rs`, `src/runtime/*`, `Cargo.toml`

## Design (Tokio three-part model)
1. **When:** `tokio::signal::ctrl_c()` + Unix `SIGTERM` via `tokio::signal::unix`.
2. **Tell:** a parent `tokio_util::sync::CancellationToken`; each worker gets a
   `child_token()` and `select!`s task execution against `token.cancelled()`.
3. **Wait:** `tokio_util::task::TaskTracker` to await drain within a deadline.

## Tasks
- [ ] Add `tokio-util` (features `rt`) dependency
- [ ] Thread a `CancellationToken` through `WorkerPool` and `ResourcePool`
- [ ] Worker loop: `select!` { task, cancelled }; finish-or-abandon per policy
- [ ] `shutdown(timeout)` → drain-with-deadline, then cancel; never block in `Drop`
- [ ] `shutdown_on_signal()` helper wiring ctrl_c + SIGTERM
- [ ] Resolve R2 policy: **drain-then-cancel** default, configurable
- [ ] Docs + example: Ctrl-C drains in-flight inference then exits

## Acceptance
- [ ] Ctrl-C during active work drains within deadline, then exits cleanly
- [ ] No detached-thread workaround needed; `Drop` only signals
- [ ] Documented cancel-vs-drain policy
