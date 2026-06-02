# CR-02 — Make `shutdown()` join honor its timeout

- **Track:** A · **Severity:** 🟠 High · **Depends on:** CR-01
- **Skill:** `async-patterns` · **Agent:** general-purpose
- **Files:** `src/core/worker_pool/native.rs` (shutdown, 449-494)

## Problem
On the "detach" branch, `shutdown()` still runs an unconditional
`let _ = join_thread.join();`, which blocks on the helper that is blocked on
`worker.join()` — the timeout/detach is illusory and can hang on a wedged worker.

## Tasks
- [x] Restructure join: join only when the worker exited; drop/detach on timeout
- [ ] Avoid spawning N helper threads — superseded by CR-03 `TaskTracker` (deferred to CR-03)
- [x] Log detached workers at WARN with worker id
- [x] Unit test: wedged worker → `shutdown()` returns within bound (`test_shutdown_returns_within_bound_with_wedged_worker`)

## Acceptance
- [x] `shutdown()` returns within per-worker timeout even with a stuck worker (~2.1s, not 30s)
- [x] `test_graceful_shutdown` still passes; worker_pool_test suite green & exits clean
