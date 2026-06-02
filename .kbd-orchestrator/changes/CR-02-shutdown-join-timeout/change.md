# CR-02 — Make `shutdown()` join honor its timeout

- **Track:** A · **Severity:** 🟠 High · **Depends on:** CR-01
- **Skill:** `async-patterns` · **Agent:** general-purpose
- **Files:** `src/core/worker_pool/native.rs` (shutdown, 449-494)

## Problem
On the "detach" branch, `shutdown()` still runs an unconditional
`let _ = join_thread.join();`, which blocks on the helper that is blocked on
`worker.join()` — the timeout/detach is illusory and can hang on a wedged worker.

## Tasks
- [ ] Restructure join: bounded wait, then truly drop/detach the handle (no trailing join)
- [ ] Avoid spawning N helper threads if superseded by CR-03 `TaskTracker`
- [ ] Log detached workers at WARN with worker id
- [ ] Unit test: wedged worker → `shutdown()` returns within bound

## Acceptance
- [ ] `shutdown()` returns within per-worker timeout even with a stuck worker
- [ ] `test_graceful_shutdown` stays < 500ms
