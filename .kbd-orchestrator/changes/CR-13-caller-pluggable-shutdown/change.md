# CR-13 — Caller-pluggable shutdown (token injection + ShutdownPolicy + handle)

- **Track:** B · **Severity:** 🟡 Medium · **Depends on:** CR-03
- **Skill:** `rust-async-patterns`, `clean-architecture` · **Agent:** Plan → general-purpose
- **Decision record:** [decision-cancellation-policy.md](../../phases/phase-certification-remediation/decision-cancellation-policy.md)
- **Files:** `src/core/worker_pool/native.rs`, `src/core/resource_pool.rs`, `src/builders/pool_builder.rs`, `src/runtime/*`

## Rationale (researched)
Give the **caller upstream ownership of shutdown** via **signal injection + a declarative
policy + an observation hook** — NOT a library-invoked async policy callback (rejected for
reentrancy/deadlock, cancel-safety leakage, and runtime coupling at teardown). See decision
record for sources (Tokio guide, wg-async#45, RFD 400, tokio-graceful-shutdown).

## Design (adopt A, reject B)
- Pool builder accepts an **optional caller `CancellationToken`**; if absent, the pool owns one
  and returns a **`ShutdownHandle`**.
- Declarative, library-owned policy:
  ```text
  #[non_exhaustive]
  enum ShutdownPolicy {
      Drain { deadline: Duration },
      DrainThenCancel { deadline: Duration },   // DEFAULT
      CancelImmediately,
  }
  ```
- `shutdown_with(policy)` + `shutdown_on_signal()` (ctrl_c + SIGTERM).
- Per-task shutdown **reactions ride the CR-07 Hooks bus** as `LifecycleEvent`s (observe,
  never await for a decision).

## Tasks
- [ ] `ShutdownPolicy` enum + `ShutdownHandle` type (documented)
- [ ] Builder: accept external `CancellationToken`; default-create otherwise
- [ ] `shutdown_with(policy)` implementing Drain / DrainThenCancel / CancelImmediately
- [ ] Emit `Cancelled`/`Drained`/`TimedOut` lifecycle events to hooks (CR-07)
- [ ] Example: caller drives shutdown from its own supervisor token (upstream control)
- [ ] Docs: explicit "we do NOT expose an async policy callback — use a Hook" note
- [ ] (Deferred, only if requested) sync `Fn(&TaskMetadata) -> ShutdownPolicy` classifier

## Acceptance
- [ ] Caller can supply its own token and fully orchestrate shutdown upstream
- [ ] Default `DrainThenCancel` behaves identically to CR-03 default
- [ ] No async user callback in the teardown path; reactions via hooks only
