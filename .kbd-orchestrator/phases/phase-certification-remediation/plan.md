# KBD Plan — phase-certification-remediation

**Project:** `prometheus_parking_lot` (Rust library crate)
**Phase:** `phase-certification-remediation`
**Stage:** Plan (no code generated — ordered change list only)
**Inputs:** [assessment.md](assessment.md), [CERTIFICATION_REPORT.md](../../../CERTIFICATION_REPORT.md), web research (`.firecrawl/`)
**Change backend:** native KBD (no `openspec/` present)
**Goal:** Fix **100%** of certification issues **and** make the crate worthy of public
publication on crates.io — including shutdown/Ctrl-C, rich error reporting, mailbox
hardening, an extensibility/hooks architecture, documentation, and a publishing playbook.
**Planned:** 2026-06-02

---

## 1. Research-Backed Design Decisions

These decisions, grounded in the web research, drive the changes below. Citations are the
authoritative sources surfaced during planning.

| Topic | Decision | Source |
|-------|----------|--------|
| Graceful shutdown | Adopt Tokio's **three-part model**: (1) decide *when* to shut down, (2) *tell* every worker/task to stop, (3) *wait* for them to finish. | [tokio.rs/tokio/topics/shutdown](https://tokio.rs/tokio/topics/shutdown) |
| Shutdown trigger | Use `tokio::signal::ctrl_c()` (+ SIGTERM via `tokio::signal::unix`) feeding a **`tokio_util::sync::CancellationToken`**; never block in `Drop`. | [Tokio shutdown guide](https://tokio.rs/tokio/topics/shutdown), [cybernetist cancellation patterns](https://cybernetist.com/2024/04/19/rust-tokio-task-cancellation-patterns/) |
| Cancellation propagation | Parent `CancellationToken` + `child_token()` per worker; `select!` task execution against `token.cancelled()`. Track liveness with `tokio_util::task::TaskTracker`. | cybernetist cancellation patterns |
| Backpressure / queue-full | Keep **bounded** channels; treat queue-full as a **typed, information-rich error** (not a panic), with depth/capacity and optional retry hint. | [tokio mpsc docs](https://docs.rs/tokio/latest/tokio/sync/mpsc/), [Handling Backpressure in Async Rust](https://medium.com/@adamszpilewicz/handling-backpressure-in-async-rust-build-scalable-systems-without-overload-28ff71945fa0) |
| Error design | `thiserror` typed enums with `#[source]` chains; one unified public error taxonomy; `#[non_exhaustive]` for forward-compat. | [Crate publishing tips](https://users.rust-lang.org/t/blog-post-crate-publishing-tips-and-guidelines/39391) |
| Hooks/extensibility | **Observer via trait objects** + extension traits; generalize the existing `AuditSink` into a lifecycle `Hook`/event bus. | [Observer pattern in Rust](https://users.rust-lang.org/t/observer-patterns-in-rust/86481), extension-trait idiom |
| Publishing | Follow the community **release checklist** + docs.rs metadata; dual-license files must exist; doctests + `#![deny(missing_docs)]` already enforced. | [Crate release checklist](https://www.reddit.com/r/rust/comments/qw5ea1/), [docs.rs/about](https://docs.rs/about) |

### Rust skills applied per track
- **Track A (blockers):** `async-patterns`, `rust-async-patterns`, `error-handling`
- **Track B (robustness):** `async-patterns`, `actor-model`, `error-handling`, `clean-architecture`
- **Track C (publish):** `coding-guidelines`, `rust-best-practices`, `workspace-structure`, `unsafe-checker`, `prometheus-rust-auditor`

---

## 2. Ordered Change List

Three tracks. **Track A is the critical path** (unblocks certification + coverage). Track B
layers in the elegance/robustness the user requested. Track C makes it publishable. Within a
track, IDs are dependency-ordered.

| ID | Title | Track | Severity | Depends on | Skill / Agent |
|----|-------|-------|----------|------------|---------------|
| **CR-01** | Fix `retrieve_async` blocking-thread leak (the cert hang) | A | 🔴 Critical | — | `async-patterns` / general-purpose |
| **CR-02** | Make `shutdown()` join honor its timeout | A | 🟠 High | CR-01 | `async-patterns` / general-purpose |
| **CR-03** | Cancellation token + Ctrl-C/SIGTERM graceful shutdown | A/B | 🟠 High | CR-02 | `rust-async-patterns` / Plan→general-purpose |
| **CR-04** | Unblock & run coverage (`cargo llvm-cov`) + suite watchdog | A | 🟡 Medium | CR-01..03 | `rust-best-practices` / general-purpose |
| **CR-05** | Unified, information-rich error taxonomy (queue-full detail) | B | 🟠 High | CR-01 | `error-handling` / general-purpose |
| **CR-06** | Mailbox hardening (trait, delivery semantics, TTL, errors) | B | 🟠 High | CR-05 | `actor-model` / general-purpose |
| **CR-07** | Extensibility & hooks architecture (lifecycle event bus) | B | 🟡 Medium | CR-05 | `clean-architecture` / Plan→general-purpose |
| **CR-08** | Fix crate metadata + dual-license files (publish blocker) | C | 🔴 Critical | — | `workspace-structure` / general-purpose |
| **CR-09** | Documentation pass: rustdoc, doctests, README/CHANGELOG sync | C | 🟠 High | CR-01..07 | `coding-guidelines` / general-purpose |
| **CR-10** | CI + release gates (fmt, clippy, msrv, doc, deny, semver) | C | 🟠 High | CR-08,09 | `rust-best-practices` / general-purpose |
| **CR-11** | Publishing playbook `docs/PUBLISHING.md` | C | 🟢 Done-now | — | `workspace-structure` / (authored in this plan) |
| **CR-12** | Final publish-readiness audit + `cargo publish --dry-run` | C | 🟠 High | all | `prometheus-rust-auditor` / general-purpose |
| **CR-13** | Caller-pluggable shutdown (token injection + `ShutdownPolicy` + handle) | B | 🟡 Medium | CR-03 | `rust-async-patterns` / Plan→general-purpose |
| **CR-14** | Cancellation capture & replay (`CancellationSink` + bounded handoff + DLQ/nack) | B | 🟠 High | CR-03,13,06,07 | `actor-model` / Plan→general-purpose |

> **CR-11 is delivered as part of this Plan phase** (see `docs/PUBLISHING.md`). All other
> changes are queued for `/kbd-execute`.

---

## 3. Change Detail (acceptance criteria + task breakdown)

Full task lists live in `.kbd-orchestrator/changes/<id>/change.md`. Summary here.

### CR-01 — Fix `retrieve_async` blocking-thread leak 🔴
**Root cause:** untimed `condvar.wait()` inside `spawn_blocking`, entry removed before
`store()` notifies → leaked blocking task → tokio runtime drop hangs.
**Fix:** replace the inner wait with a bounded `wait_for(remaining)` **or** switch the
result path to a `tokio::sync::oneshot` signaled by `store()`; guarantee every waiter is
woken on removal (set a `Cancelled` state + `notify_all()`).
**Accept:** `test_timeout_handling` and the full `worker_pool_test` suite exit cleanly with
no `kill`; no leaked blocking-pool threads; blocking `retrieve` behavior unchanged.

### CR-02 — `shutdown()` honors its timeout 🟠
Remove the unconditional trailing `join_thread.join()` on the detach branch; restructure to
a single bounded join. **Accept:** `shutdown()` returns within its per-worker bound even with
a wedged worker.

### CR-03 — Cancellation + Ctrl-C/SIGTERM 🟠
Add `tokio-util` `CancellationToken`; worker loop `select!`s task execution vs.
`token.cancelled()`; `WorkerPool`/`ResourcePool` expose `shutdown(timeout)` + a
`shutdown_on_signal()` helper wiring `tokio::signal::ctrl_c()` and Unix SIGTERM. Track tasks
with `TaskTracker`. `Drop` only *signals* (never blocks). **Accept:** Ctrl-C drains in-flight
work within the deadline then exits; documented cancel-vs-drain policy (resolves assessment
**R2**). **Amended:** constructors/builders accept an *optional caller-supplied*
`CancellationToken`; the caller-facing surface is specified in **CR-13**.

### CR-13 — Caller-pluggable shutdown 🟡
**Researched decision** ([decision-cancellation-policy.md](decision-cancellation-policy.md)):
expose an **outgoing interface for upstream shutdown control** as **signal injection + a
declarative `ShutdownPolicy` enum + a `ShutdownHandle`** — and **reject** a library-invoked
*async policy callback* per in-flight task (reentrancy/deadlock, cancel-safety leakage,
runtime coupling at teardown). Per-task shutdown *reactions* ride the **CR-07 Hooks bus** as
events, never an awaited control callback. **Accept:** a caller can supply its own token and
fully orchestrate shutdown upstream; default `DrainThenCancel` matches CR-03; no async user
callback in the teardown path.

### CR-14 — Cancellation capture & replay 🟠
**Researched decision** ([decision-cancellation-capture.md](decision-cancellation-capture.md)):
AI-inference tasks are cancellable (no always-drain), so cancelled work must be **captured for
replay**. Adopt the **DLQ + bounded-handoff + at-least-once nack** triad: (A) build an owned
`CancelledTask<P>` envelope (payload snapshot + meta + `CancelReason` + attempt); (B) hand it to
a caller-supplied **`CancellationSink<P>` trait** *off the teardown path* via a bounded mpsc,
invoked under a per-call `timeout(delta)` with a guaranteed fallback sink (so it can never hang
the app); (C) make persistent backends **replayable via nack/redelivery** with a documented
idempotency contract. A `Cancelled` event also fires on the CR-07 hooks bus. **Accept:**
cancelled inference tasks captured with full owned context; sink invocation provably bounded;
persistent backends redeliver requeued tasks.

### CR-04 — Coverage + watchdog 🟡
Run `cargo llvm-cov` (profile exists), record baseline %, add a suite-level wall-clock
watchdog so future hangs fail loudly. **Accept:** coverage report generated for all 6 suites;
baseline committed.

### CR-05 — Unified error taxonomy 🟠
Collapse `PoolError` + `SchedulerError` into one `#[non_exhaustive]` public error enum (or a
bridged hierarchy) using `thiserror` with `#[source]` chains. **QueueFull** carries
`{ pool, depth, capacity, retry_after: Option<Duration> }`. Document every variant.
**Accept:** one documented public error type; queue-full reports actionable detail; no
stringly-typed `Backend(String)` without a `#[source]`.

### CR-06 — Mailbox hardening 🟠
Define an explicit public `Mailbox<T>` contract: delivery semantics (at-least-once),
optional TTL/expiry + eviction, typed errors, notifier failure handling, idempotent
retrieval. Audit `InMemory`/`Postgres`/`Yaque` impls against it. **Accept:** documented
trait + semantics; all three backends conform; tests for expiry, redelivery, notifier error.

### CR-07 — Hooks / extensibility 🟡
Generalize `AuditSink` into a lifecycle `Hook` trait with events
(`Submitted`/`Queued`/`Started`/`Completed`/`Failed`/`Evicted`/`Rejected`); allow N
registered observers; keep zero-cost when none registered. **Accept:** documented hooks API;
example custom hook; audit refactored onto it without behavior change.

### CR-08 — Metadata + license 🔴 (publish blocker)
`Cargo.toml` currently describes a *synchronization primitives* fork (wrong
`description`/`keywords`/`categories`). Rewrite to describe the **scheduler**; add
`readme`, `homepage`, `documentation`, accurate `categories`
(`asynchronous`, `concurrency`), `keywords` (`scheduler`, `queue`, `ai`, `gpu`, `tokio`).
Add **`LICENSE-APACHE`** (declared `MIT OR Apache-2.0` but only `LICENSE-MIT` exists).
**Accept:** `cargo publish --dry-run` metadata clean; both license files present.

### CR-09 — Documentation pass 🟠
Crate- and item-level rustdoc completeness (deny gate already on), runnable doctests
(replace `ignore` examples where feasible), `#![doc = include_str!("../README.md")]` or
sync, `docs/` pruned of stale fork-era files, CHANGELOG updated. **Accept:**
`cargo doc` clean; doctests pass; README example compiles.

### CR-10 — CI + release gates 🟠
GitHub Actions: `fmt --check`, `clippy --all-targets -D warnings` (pedantic/nursery), test
matrix, MSRV (1.91.1) check, `cargo doc`, `cargo-deny`, optional `cargo-semver-checks`,
`llvm-cov` upload. **Accept:** green CI on a PR; gates enforced.

### CR-11 — Publishing playbook ✅ (delivered now)
`docs/PUBLISHING.md` — see deliverable. Step-by-step crates.io release runbook.

### CR-12 — Final audit 🟠
Run `prometheus-rust-auditor` + `unsafe-checker` (unsafe already denied), `cargo publish
--dry-run`, `cargo package` inspection, public-API review. **Accept:** dry-run succeeds;
audit clean; tag + version set.

---

## 4. Sequencing & Milestones

```
Milestone 1 — CERTIFY (unblock):     CR-01 → CR-02 → CR-04           [green, non-hanging suite + coverage]
Milestone 2 — HARDEN (elegance):     CR-03 → CR-13 → CR-14, CR-05 → CR-06, CR-07   [shutdown/Ctrl-C, caller policy, cancel-capture/replay, errors, mailbox, hooks]
Milestone 3 — PUBLISH (worthy):      CR-08 ∥ CR-09 → CR-10 → CR-12   [metadata, docs, CI, dry-run]
                                     (CR-11 done in this plan)
```
CR-01 and CR-08 can start immediately in parallel (independent). CR-03 supersedes the
stopgap in CR-02 with the real cancellation architecture.

## 5. Resolved / Open Questions
- **R2 (RESOLVED by user):** **Do NOT always-drain** — AI-inference tasks can hang, so
  stragglers past the deadline are **cancelled**, *because* cancelled work is now **captured
  for replay** via **CR-14**. Default policy stays `DrainThenCancel { deadline }`; per-pool
  override via CR-13. See [decision-cancellation-capture.md](decision-cancellation-capture.md).
- **R5 (resolved):** *Should the caller plug in a custom cancellation policy?* → **Yes, in a
  bounded form** — accept a caller `CancellationToken` + declarative `ShutdownPolicy` +
  `ShutdownHandle` (**CR-13**); **reject** a library-invoked async policy callback. Full
  rationale + sources in [decision-cancellation-policy.md](decision-cancellation-policy.md).
- **R3:** verify the WASM worker path doesn't mirror the CR-01 leak (folded into CR-01).

## 6. Recommended next step
Run `/kbd-execute phase-certification-remediation` starting at **Milestone 1 (CR-01)**.
Execute CR-08 in parallel since it is independent and unblocks the publish track early.
