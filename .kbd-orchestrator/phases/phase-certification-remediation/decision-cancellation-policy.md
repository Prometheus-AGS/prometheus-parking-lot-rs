# Decision Record — Caller-pluggable cancellation / shutdown policy

**Question:** Should the crate expose a hook / outgoing interface that lets the **caller**
implement the cancellation policy and own shutdown details upstream?

**Status:** ✅ Recommended — **adopt a bounded form** (signal injection + declarative policy +
handle). ❌ Reject the heavyweight form (library-invoked async policy callback per task).
**Date:** 2026-06-02 · **Informs:** CR-03, CR-07 · **New change:** CR-13

---

## 1. The key distinction the research surfaces

Two designs get conflated under "let the caller control shutdown." They have very different
risk profiles:

| | **A. Signal injection (IoC)** | **B. Policy callback (library calls user code)** |
|---|---|---|
| Shape | Caller passes in a `CancellationToken` (or gets a `ShutdownHandle` back) + picks a declarative policy | Caller implements `trait CancellationPolicy { async fn decide(&self, task) -> Action }` that the **library awaits per in-flight task during teardown** |
| Who decides *when* | Caller (upstream) | Caller |
| Who decides *how* | Caller, from a fixed menu | Caller, via arbitrary async code |
| Runtime coupling | None — caller owns orchestration | Library awaits user futures at the most fragile moment |
| Idiomatic? | **Yes** — this is the documented Tokio pattern | Rare; fights the language |

## 2. What the sources say

- **Tokio's official guide** makes `CancellationToken` *the* mechanism for "tell tasks to shut
  down," and shows **libraries accepting a token** rather than owning the signal.
  → IoC (A) is the blessed path. ([tokio.rs/tokio/topics/shutdown](https://tokio.rs/tokio/topics/shutdown))
- **wg-async #45** ("writing a library that can be reused across many runtimes") and the
  **internals cancellation design doc** argue libraries should **accept cancellation signals,
  not own the runtime / shutdown lifecycle**. → reinforces (A), warns against (B)'s coupling.
  ([wg-async#45](https://github.com/rust-lang/wg-async/issues/45), [internals](https://internals.rust-lang.org/t/async-await-the-challenges-besides-syntax-cancellation/10287))
- **`tokio-graceful-shutdown`** author explicitly found that *token propagation alone is
  insufficient* and built a richer **subsystem/`ShutdownGuard`** model — but note: that model
  is **observe-and-coordinate** (guards, nested subsystems), **not** a per-task async callback
  the library invokes to make decisions. → richer ergonomics are valuable, but as *handles*,
  not control callbacks. ([crate](https://crates.io/crates/tokio-graceful-shutdown))
- **Async-callback pitfalls** (cancel-safety, `FnMut`/async-closure friction, reentrancy and
  lock-holding deadlocks) are well documented and especially dangerous **during teardown**,
  when locks are held and futures are being dropped. Oxide's **RFD 400** and Tyler Mandry's
  *Making Async Rust Reliable* stress that async cancellation has **implicit contracts** that
  arbitrary user callbacks routinely violate.
  → (B) imports exactly these footguns into the most fragile code path.
  ([RFD 400](https://rfd.shared.oxide.computer/rfd/0400), [making-async-reliable](https://tmandry.gitlab.io/blog/posts/making-async-reliable/), [without.boats async clean-up](https://without.boats/blog/asynchronous-clean-up/))

## 3. Why a per-task async policy callback (B) is unwise here

1. **Reentrancy / deadlock.** The library would call the caller's future while holding pool
   internals (the `task_tx` mutex, result-map locks). If that future touches the pool, you
   deadlock — precisely the class of bug CR-01/CR-02 are fixing.
2. **Cancel-safety contract leaks to the user.** The callback runs inside a deadline-bounded
   `select!`; if it's dropped at an await point it must be cancel-safe. Most users can't
   guarantee that, so the library inherits their bugs.
3. **Teardown is the worst place for arbitrary work.** Drop ordering, half-cancelled tasks,
   and runtime-shutdown races make user code here fragile and hard to test.
4. **Runtime coupling.** Awaiting user futures during shutdown ties the library's teardown to
   the caller's runtime state — the opposite of the "runtime-agnostic" goal in CLAUDE.md.

## 4. Why signal injection + declarative policy (A) is the right "outgoing interface"

It gives the caller **upstream ownership of shutdown** — exactly what was asked — without the
callback hazards:

- **Inject the signal.** `WorkerPool`/`ResourcePool` accept a caller-supplied
  `CancellationToken` (or expose a `ShutdownHandle`). The caller wires `ctrl_c`, SIGTERM,
  Kubernetes preStop, a parent subsystem token, etc., entirely upstream and triggers shutdown
  whenever it wants. The pool simply observes the token.
- **Choose the policy declaratively.** A small, closed enum the library fully controls:
  ```text
  enum ShutdownPolicy {
      Drain { deadline: Duration },           // finish in-flight, reject new, wait up to deadline
      DrainThenCancel { deadline: Duration }, // drain; cancel stragglers past deadline  (DEFAULT)
      CancelImmediately,                      // signal cancel now, best-effort
  }
  ```
  This covers the real use-cases (graceful drain, bounded drain, hard stop) with **zero async
  user code in the teardown path**.
- **Observe via Hooks, don't control via callback.** Per-task shutdown *visibility*
  (e.g. "task X was cancelled / drained / timed out") is delivered as **`LifecycleEvent`s on
  the CR-07 Hooks bus** — fire-and-observe, never awaited for a decision. Callers that need
  custom side-effects (flush a buffer, emit a metric, persist partial state) do them in a hook
  reacting to `Started`/`Cancelled`/`Completed`, not in the shutdown control loop.

This is the same separation mature crates use: **handles + tokens for control, observers for
reactions.**

## 5. Recommendation

**Adopt design A. Reject design B.** Concretely:

1. **CR-03 (amended):** pool constructors/builders accept an optional caller
   `CancellationToken`; if none is supplied, the pool creates its own and exposes a
   `ShutdownHandle`. Add `shutdown_with(policy)` plus the `ShutdownPolicy` enum; default
   `DrainThenCancel`.
2. **CR-13 (new):** the caller-pluggable shutdown surface — token injection, `ShutdownPolicy`,
   `ShutdownHandle`, and `shutdown_on_signal()`. Documented with an example showing a caller
   driving shutdown from its own supervisor/runtime.
3. **CR-07 (unchanged scope, explicit link):** per-task shutdown reactions ride the Hooks bus
   as events; **no async policy callback** is exposed.

**Escape hatch (only if a real need appears):** if a user ever needs genuine per-task decision
logic, add a **synchronous, non-reentrant, `#[non_exhaustive]`-returning** classifier
`Fn(&TaskMetadata) -> ShutdownPolicy` — pure, fast, no `async`, no pool access — so they can
vary policy *by task class* without entering the async-callback danger zone. Keep this out of
v1 unless requested.

## 6. Net

The user's instinct is right — **expose an outgoing interface for upstream shutdown control** —
but the wise form is **signal injection + a declarative policy + an observation hook**, not a
library-invoked async cancellation callback. Added to the plan as **CR-13**, with CR-03
amended and CR-07 cross-linked.
