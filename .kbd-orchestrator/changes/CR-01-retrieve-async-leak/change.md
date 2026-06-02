# CR-01 — Fix `retrieve_async` blocking-thread leak

- **Track:** A (certification blocker) · **Severity:** 🔴 Critical · **Depends on:** none
- **Skill:** `async-patterns` · **Agent:** general-purpose
- **Files:** `src/core/worker_pool/native.rs`, `src/core/worker_pool/wasm.rs` (audit)

## Problem
`retrieve_async` (native.rs:386-410) spawns a blocking task that calls
`condvar.wait()` with **no timeout**; the result entry is removed on the outer timeout
before `store()` can notify, so the blocking task parks forever and wedges tokio runtime
teardown → the certification test-runner hang in `test_timeout_handling`.

## Approach
Option A (preferred): replace the result-delivery path with a `tokio::sync::oneshot`
created in `create_slot` and fired by `store`; `retrieve_async` awaits it under
`tokio::time::timeout` — no blocking thread at all.
Option B: keep Condvar but use bounded `wait_for(remaining)`, and on `remove` set a
`Cancelled` state + `notify_all()` so no waiter is ever orphaned.

## Tasks
- [ ] Choose oneshot vs. bounded-wait (document rationale in code)
- [ ] Implement woken-on-removal guarantee (no orphaned waiters)
- [ ] Keep blocking `retrieve`/`wait_for_result` behavior identical
- [ ] Audit `wasm.rs` for the same anti-pattern (R3)
- [ ] Add regression test: slow executor + short retrieve timeout exits clean

## Acceptance
- [ ] `cargo test --test worker_pool_test` exits with no `kill`/hang
- [ ] No leaked blocking-pool threads after the suite
- [ ] `test_timeout_handling` still returns `PoolError::Timeout`
