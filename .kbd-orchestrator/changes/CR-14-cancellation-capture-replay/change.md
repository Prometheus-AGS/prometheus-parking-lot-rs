# CR-14 — Cancellation capture & replay (`CancellationSink` + bounded handoff + DLQ/nack)

- **Track:** B · **Severity:** 🟠 High · **Depends on:** CR-03, CR-13, CR-06, CR-07
- **Skill:** `actor-model`, `rust-async-patterns`, `error-handling` · **Agent:** Plan → general-purpose
- **Decision records:** [decision-cancellation-capture.md](../../phases/phase-certification-remediation/decision-cancellation-capture.md), [decision-idempotency.md](../../phases/phase-certification-remediation/decision-idempotency.md)

## Locked idempotency semantics (user-confirmed 2026-06-02)
- **D1** Add `idempotency_key: Option<String>` to `TaskMetadata`; routing/result key derives
  from it. `submit()` must stop ignoring caller identity. `None` ⇒ unique, no dedup/replay.
- **D2** At-least-once; **executor must be idempotent** on the key. Library does NOT do exactly-once.
- **D3** Auto-requeue only `QueueShutdown`/`DrainTimeout`; `DeadlineExpired`/`SignalCancelled` ⇒ DLQ-only.
- **D4** `max_attempts = 3` default + exponential backoff (both per-pool config); exhausted ⇒ terminal DLQ.
- **D5** Results first-write-wins + consume-once, keyed by `idempotency_key`.
- **D6** Partial side-effects on replay = documented caller contract (resume-vs-restart via `CancelledTask`).
- **D7** Persist `idempotency_key` + `attempt` in queue metadata; in-memory = no cross-restart guarantee.
- **Files:** `src/core/worker_pool/native.rs`, `src/core/resource_pool.rs`, `src/core/hooks.rs` (CR-07),
  `src/infra/queue/*` (nack/redelivery), `src/builders/pool_builder.rs`

## Goal
AI-inference tasks are cancellable (no always-drain). When cancelled, **capture the owned task
context and hand it to the caller via a trait, bounded by a deadline**, and make persistent
backends **replayable**. Idempotent by contract.

## Design (3 layers — see decision record)
- **A. Capture:** `CancelledTask<P> { payload, meta, reason: CancelReason, attempt, cancelled_at_ms }`
  (`CancelReason` = `DrainTimeout|DeadlineExpired|SignalCancelled|QueueShutdown`). Retain a
  payload snapshot at submit so interrupted in-flight work is reconstructable.
- **B. Hand back (bounded, decoupled):** `#[async_trait] trait CancellationSink<P>` with
  `async fn on_cancelled(&self, CancelledTask<P>) -> SinkOutcome` (`Recorded|Requeue|Drop`).
  Pool `try_send`s to a bounded mpsc; a dedicated drain task calls the sink under
  `timeout(delta)`; overrun/full ⇒ default fallback (log + optional built-in DLQ). **Never**
  awaited inline on the teardown path.
- **C. Replay:** `Requeue` ⇒ **nack** on persistent queue (don't delete; bump `attempts`; set
  redelivery delay). In-memory ⇒ caller sink is the only durability. Fire `Cancelled` hook
  event (CR-07) for observability.

## Tasks
- [ ] Add `idempotency_key: Option<String>` to `TaskMetadata`; route on it (stop ignoring caller identity) [D1]
- [ ] Define `CancelledTask<P>`, `CancelReason`, `SinkOutcome` (documented, `#[non_exhaustive]`)
- [ ] Retain payload snapshot at submit for in-flight reconstruction
- [ ] Per-`CancelReason` routing: env→requeue, deadline/signal→DLQ [D3]
- [ ] `max_attempts` + exponential backoff config; exhausted→terminal DLQ [D4]
- [ ] Result store first-write-wins + consume-once keyed by idempotency_key [D5]
- [ ] Persist idempotency_key + attempt in queue metadata (JSONB) [D7]
- [ ] `CancellationSink<P>` trait + default logging/DLQ sink
- [ ] Bounded mpsc handoff + dedicated drain task with per-call `timeout(delta)` + fallback
- [ ] Builder: `with_cancellation_sink(...)` + configurable `delta`
- [ ] Persistent-queue nack/redelivery path (Postgres/pgmq/Yaque); attempts + redelivery delay
- [ ] Emit `Cancelled` lifecycle event on the hooks bus
- [ ] Document idempotency contract: at-least-once, dedupe on `idempotency_key`, executor must be idempotent [D2/D6]
- [ ] Tests: deadline-expiry capture, shutdown-straggler capture, sink-timeout fallback,
      requeue→redelivery, full-channel fallback, idempotent replay

## Acceptance
- [ ] Cancelled inference tasks are captured with full owned context
- [ ] Caller sink is invoked off the teardown path and **cannot hang** the app (per-call delta
      + fallback proven by test)
- [ ] Persistent backends redeliver requeued tasks; in-memory documents the limitation
- [ ] R2 default = cancel-stragglers-then-capture (no always-drain)
