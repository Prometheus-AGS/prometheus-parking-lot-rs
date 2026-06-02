# KBD Assessment — phase-certification-remediation

**Project:** `prometheus_parking_lot` (Rust library crate, edition 2021, rustc 1.91.1)
**Phase:** `phase-certification-remediation`
**Stage:** Assess (no code generated — analysis only)
**Source of record:** [CERTIFICATION_REPORT.md](../../../CERTIFICATION_REPORT.md) (dated Dec 7, 2025)
**Assessed:** 2026-06-02

---

## 1. Executive Summary

The certification report declares core functionality and the candle-vllm integration
**passing** (27 green tests across 3 suites) but flags one **production blocker**: the
`worker_pool_test.rs` suite **hangs the test runner after all tests report PASSED**, and
two follow-on gaps (coverage analysis incomplete, shutdown mechanism "needs refinement").

This assessment **reproduced the hang empirically** and isolated it to a **single
deterministic root cause** plus two latent defects in the same module
([src/core/worker_pool/native.rs](../../../src/core/worker_pool/native.rs)). The report
characterizes the issue vaguely ("detached worker threads"); the real defect is a
**leaked blocking thread in `retrieve_async` that wedges tokio runtime teardown**. The
report's suggested fix (making workers daemon threads) would **mask but not cure** it.

**Verdict:** The crate is **not production-ready** as stated, but the blocker is **small,
well-localized, and low-risk to fix**. Estimated remediation: **1 focused change set**
(~1–2 days incl. verification). No architectural redesign required.

---

## 2. Issue Inventory (from CERTIFICATION_REPORT.md)

| # | Issue (as reported) | Report severity | Verified? | True severity |
|---|---------------------|-----------------|-----------|---------------|
| I1 | `worker_pool_test.rs` hangs test runner after all tests complete | ⚠️ Blocker | ✅ Reproduced | 🔴 **Critical** |
| I2 | "WorkerPool shutdown mechanism needs refinement" / detached threads | ⚠️ Caution | ✅ Confirmed in code | 🟠 **High** |
| I3 | Coverage analysis not complete (blocked by I1) | ⏳ Pending | ✅ Confirmed blocked | 🟡 **Medium** |
| I4 | Recommendation: "make worker threads daemon threads for tests" | Proposed fix | ⚠️ Misdiagnosis | 🟡 **Reject as primary fix** |

---

## 3. Reproduction & Root-Cause Analysis

### 3.1 Reproduction (confirmed)

Running `cargo test --test worker_pool_test -- --test-threads=1 --nocapture` and killing
after 50s: **every test prints `PASSED` and `TEST_END`**, but the process hangs. The hang
occurs **immediately after `test_timeout_handling` completes its body** — its `PASSED`
banner prints, but libtest never emits the trailing `ok`, and the runner never advances to
the next test. So the hang is in **test teardown**, not in any test body. (Each test wraps
its async body in a `with_timeout` future guard — but that guard only cancels the *future*;
it cannot interrupt **OS worker threads** or **blocking-pool threads**, so it does not and
cannot catch this hang.)

### 3.2 Primary root cause — leaked blocking thread in `retrieve_async`  🔴

In [`native.rs:386-410`](../../../src/core/worker_pool/native.rs), `retrieve_async` waits
for a result like this:

```rust
let result = tokio::time::timeout(timeout, async move {
    tokio::task::spawn_blocking(move || {
        let (entry_mutex, condvar) = entry_pair.as_ref();
        let mut entry = entry_mutex.lock();
        if entry.state == ResultState::Ready { return entry.result.take(); }
        condvar.wait(&mut entry);          // ← (A) NO TIMEOUT on the inner wait
        ...
    }).await.ok().flatten()
}).await;
self.results.remove(&key_clone);            // ← (B) entry removed on timeout
```

The defect is the interaction of **(A)** and **(B)**:

1. `test_timeout_handling` submits a task to a **500 ms** `SlowExecutor`, then calls
   `retrieve_async(key, 100ms)`.
2. The worker thread is busy inside `rt.block_on(executor.execute(...))` (sleeping 500 ms),
   so the result is **not yet stored**.
3. At 100 ms the **outer** `tokio::time::timeout` fires and returns `Err(Timeout)` — correct
   behavior, test passes. But the **inner `spawn_blocking` closure is still parked on
   `condvar.wait()`**, which has **no timeout**.
4. The outer future then runs `(B) self.results.remove(key)`, deleting the map entry.
5. At 500 ms the worker finishes and calls `results.store(key, ...)`
   ([`native.rs:81-95`](../../../src/core/worker_pool/native.rs)). `store` looks the key up
   in the map — **but it was already removed in step 4** — so it finds nothing and **never
   calls `condvar.notify_all()`**.
6. The `spawn_blocking` thread is now parked on `condvar.wait()` **forever**.
7. When the `#[tokio::test]` runtime is dropped at end of test, runtime shutdown **waits for
   in-flight blocking-pool tasks to finish**. The parked task never finishes → **runtime
   drop blocks → the test never returns → the runner hangs.**

This is deterministic and fully explains why the hang lands precisely on the one test that
combines a slow executor with a short retrieve timeout.

### 3.3 Secondary defect — `shutdown()` join logic defeats its own timeout  🟠

[`native.rs:467-491`](../../../src/core/worker_pool/native.rs): `shutdown()` spawns a helper
thread per worker to `join()` with a 2 s `recv_timeout`, intending to **detach** stuck
workers. But after the match it **unconditionally** runs:

```rust
let _ = join_thread.join();   // blocks until worker.join() returns
```

On the timeout/"detaching" branch the helper is still blocked on `worker.join()`, so this
final `join_thread.join()` **blocks forever** — the "detach" is illusory. Today this rarely
triggers (idle workers exit fast once the sender drops), but **any worker stuck in an
uninterruptible executor will hang `shutdown()`**. Latent landmine.

### 3.4 Tertiary defect — uncancellable in-flight work + leaky `Drop`  🟠

`Drop` ([`native.rs:503-515`](../../../src/core/worker_pool/native.rs)) deliberately does
**not** join workers (to avoid hangs), leaving threads to the OS. Combined with the fact
that executor work runs inside `rt.block_on(...)` with **no cancellation path**, a busy
worker cannot be stopped — only waited out. This is the architectural reason the report
reached for "daemon threads." It is a real gap but **not the cause of the certification
hang**.

### 3.5 Why the report's proposed fix (I4) is wrong

"Make worker threads daemon threads" — Rust std has **no daemon-thread concept**; detached
threads already don't block `process::exit`. The hang is **not** an OS worker thread keeping
the process alive — it is a **tokio blocking-pool task wedging runtime *drop* inside a single
test**. Daemonizing workers would not touch the `condvar.wait()` leak. **Reject I4 as the
primary remedy.**

---

## 4. Gap Assessment Against Phase Goal (production readiness)

| Dimension | Current state | Gap |
|-----------|---------------|-----|
| Functional correctness | 27/27 functional tests pass | ✅ None observed |
| Test-suite hygiene | `worker_pool_test` hangs runner; blocks CI | 🔴 Must fix (I1) |
| Shutdown determinism | `shutdown()` can self-deadlock; `Drop` leaks threads | 🟠 Must harden (I2) |
| Result-retrieval safety | `retrieve_async` leaks blocking threads on timeout | 🔴 Must fix (I1 root) |
| Coverage measurement | Not run; blocked by hang | 🟡 Unblock + run (I3) |
| Cancellation model | No way to cancel in-flight executor work | 🟠 Design gap (I2/I3.4) |
| Lint/doc gates | `unsafe_code=deny`, `missing_docs=deny`, clippy pedantic/nursery=warn | ⚪ Re-verify after fix |

---

## 5. Recommended Remediation Strategy (for the Plan phase)

Ordered by dependency. **No code in this phase** — this is the proposed change list to hand
to `/kbd-plan`.

### C1 — Fix `retrieve_async` blocking-thread leak  🔴 *(unblocks everything)*
- Replace the unbounded inner `condvar.wait(&mut entry)` with a **bounded
  `condvar.wait_for(&mut entry, remaining_timeout)`** (mirror the already-correct blocking
  `wait_for_result` at [`native.rs:114-147`](../../../src/core/worker_pool/native.rs)), **or**
  drop `spawn_blocking` entirely and await a `tokio::sync::oneshot` signaled by `store`.
- Ensure the waiter is **always woken**: either keep the entry until the waiter observes it,
  or have `remove` set a "cancelled" state and `notify_all()` so no waiter parks forever.
- **Acceptance:** `test_timeout_handling` completes and the runner exits cleanly; no leaked
  blocking-pool threads (verify the suite returns to prompt without `kill`).

### C2 — Make `shutdown()` join honor its timeout  🟠
- On the detach branch, **do not** call `join_thread.join()` (drop/`detach` the handle), or
  restructure to a single bounded join. Eliminate the unconditional trailing join.
- **Acceptance:** `shutdown()` returns within its 2 s/worker bound even if a worker is wedged.

### C3 — Introduce a cancellation/timeout path for in-flight executor work  🟠
- Add a shutdown signal the worker checks (e.g. `select!` between task execution and a
  shutdown `Notify`/token), so a busy worker can abandon work on shutdown instead of being
  detached. Decide policy: drain-then-exit vs. cancel-in-flight.
- **Acceptance:** a pool with a long-running executor shuts down deterministically without
  thread leaks; `Drop` no longer needs to abandon threads.

### C4 — Unblock and complete coverage analysis  🟡
- After C1, run `cargo llvm-cov` (a `[profile.coverage]` already exists in
  [Cargo.toml](../../../Cargo.toml)). Capture a baseline % and wire it into CI.
- **Acceptance:** coverage report generated for the full suite; baseline recorded.

### C5 — Harden the suite + re-verify gates  🟡
- Run the full `cargo test` (all 6 suites) green with **no per-test `kill` needed**.
- Re-run `cargo clippy --all-targets` (pedantic/nursery) and confirm `missing_docs`/
  `unsafe_code` deny-gates still pass after edits.
- Consider a CI watchdog (suite-level wall-clock) so a future regression fails loudly
  instead of hanging.

### Dependency / sequencing
```
C1 ──► C4 (coverage needs a non-hanging suite)
C1 ──► C2 ──► C3   (retrieval fix first, then shutdown determinism, then cancellation)
C1..C3 ─► C5 (final verification gate)
```
C1 is the critical path and likely a **<30-line change**. C2 is a small structural fix.
C3 is the only item with design choices (drain vs. cancel) and should get a brief decision
note in `/kbd-plan`.

---

## 6. Risks, Unknowns & Open Questions

- **R1 (low):** Fixing C1 via `oneshot` changes the retrieval path shared with the blocking
  `retrieve`; keep both APIs behavior-compatible. The blocking path (`wait_for_result`) is
  already correct and can serve as the reference.
- **R2 (med):** C3 cancellation policy is a product decision — should in-flight LLM inference
  be abandoned on shutdown, or always drained? Affects mailbox/result semantics. **Open
  question for the user/Plan phase.**
- **R3 (low):** WASM `worker_pool/wasm.rs` was **not** assessed here (report is native-only);
  confirm the same retrieval pattern isn't mirrored there.
- **R4 (low):** Other suites pass today but were not re-run under this assessment beyond
  reproduction of the hang; C5 covers full re-verification.

---

## 7. Recommendation for Planning

**Proceed to `/kbd-plan`** with the C1–C5 change list above as the ordered backlog. Treat
**C1 as the single certification blocker** — it is the actual cause of the reported hang,
is deterministic, and is low-risk to fix. C2/C3 convert the shutdown story from "works by
luck" to "deterministic by design." C4/C5 restore the coverage + green-suite evidence the
certification report was missing.

**Do not** adopt the report's daemon-thread recommendation (I4) as the fix — record it as a
rejected alternative with the rationale in §3.5.

Resolve **open question R2** (cancel vs. drain on shutdown) before C3 is planned in detail.
