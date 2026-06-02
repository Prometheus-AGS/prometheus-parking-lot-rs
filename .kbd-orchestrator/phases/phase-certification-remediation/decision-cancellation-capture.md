# Decision Record — Cancellation capture & replay of interrupted tasks

**Question:** AI-inference tasks must be **cancellable** (they can hang), so the default must
*not* be always-drain. When such a task **is** cancelled, we need to **record what was
cancelled** and hand the **call's context back to the caller** — via a trait — so the caller
can decide how to handle/replay it, **within a bounded time** so the handler can't hang the
app either. What is the right design?

**Status:** ✅ Recommended — **`CancellationSink<P>` trait + bounded decoupled handoff +
at-least-once nack/DLQ replay.** **Date:** 2026-06-02 · **Informs:** R2 (default policy),
CR-03, CR-06, CR-07, CR-13 · **New change:** CR-14

---

## 1. What the research establishes

- **Dead-letter queue (DLQ) is the canonical "capture for later replay" pattern.** Instead of
  deleting a job that failed or was interrupted, move it to a separate retained store so it
  can be inspected/re-run. River's job queue keeps a `river_job_dead_letter` table for exactly
  this; the DLQ pattern is the standard resiliency primitive across Celery/RabbitMQ, Logstash,
  Redpanda. → *record cancelled work, don't lose it.*
  ([River DLQ](https://riverqueue.com/docs/pro/dead-letter-queue), [DLQ pattern](https://andrew-jones.com/blog/the-dead-letter-queue-pattern/), [OneUptime DLQ](https://oneuptime.com/blog/post/2026-02-09-dead-letter-queue-patterns/view))
- **Replay = at-least-once redelivery (visibility timeout / nack).** SQS/Pulsar make an
  in-flight message invisible while processed and **redeliver it if not acked within the
  visibility timeout**; a **negative ack (nack)** requeues it promptly. Consumers must be
  **idempotent** because duplicates are guaranteed. → for the crate's *persistent* queues,
  cancellation should **nack/leave-unacked**, not delete, so the job is naturally replayable.
  ([SQS visibility timeout](https://docs.aws.amazon.com/AWSSimpleQueueService/latest/SQSDeveloperGuide/sqs-visibility-timeout.html), [at-least-once](https://oneuptime.com/blog/post/2026-01-30-at-least-once-delivery/view), [Pulsar acks/nacks](https://streamnative.io/blog/at-least-once-exactly-once-and-acks-in-pulsar))
- **You cannot run arbitrary async user code safely in the teardown/Drop path.** There is no
  async `Drop`; cancellation drops the future's owned values; awaiting cleanup at a cancel
  point is a cancel-correctness minefield. → the caller's handler must be **decoupled from the
  teardown hot path** and **deadline-bounded**, never awaited inline while pool locks are held.
  ([Without Boats — async clean-up](https://without.boats/blog/asynchronous-clean-up/), [RFD 400](https://rfd.shared.oxide.computer/rfd/0400), [Rain — Cancelling Async Rust](https://www.youtube.com/watch?v=zrv5Cy1R7r4))

## 2. How this reconciles with CR-13's "reject async callback"

CR-13 rejected a library-invoked **async control callback in the teardown hot path** (it would
hold pool locks, decide control flow, and could deadlock/reenter). The handler proposed here is
a **different animal** and is safe because it is:

1. **Off the teardown path** — the pool only *captures* an owned envelope and pushes it to a
   bounded channel; it never awaits user code while holding locks.
2. **Deadline-bounded per call** — a dedicated drain task invokes the sink under
   `tokio::time::timeout(delta)`; overruns fall back to a default sink.
3. **A data sink, not a control decision** — it records/persists; it does not steer the pool's
   shutdown control flow.

So: *control* stays declarative (CR-13 `ShutdownPolicy`); *capture* gets a real trait (here).

## 3. Recommended design — three layers

### Layer A — Capture an owned context envelope
On cancellation (shutdown straggler **or** deadline-expiry), the pool emits:
```text
struct CancelledTask<P> {
    payload: P,                 // the original job, owned -> replayable
    meta: TaskMetadata,         // id, priority, cost, mailbox, deadline
    reason: CancelReason,       // DrainTimeout | DeadlineExpired | SignalCancelled | QueueShutdown
    attempt: u32,
    cancelled_at_ms: u64,
}
#[non_exhaustive] enum CancelReason { DrainTimeout, DeadlineExpired, SignalCancelled, QueueShutdown }
```
Implementation note: retain the submitted payload (it is `Serialize + DeserializeOwned` per
`TaskPayload`, so a snapshot/clone is cheap) so an interrupted in-flight task can still be
reconstructed even though the executor took ownership of its working copy.

### Layer B — Hand back to the caller via a bounded, decoupled trait
```text
#[async_trait]
trait CancellationSink<P>: Send + Sync {
    /// Handle a cancelled task. MUST return within the pool's configured `delta`;
    /// overruns are abandoned to the fallback sink. Should be idempotent.
    async fn on_cancelled(&self, task: CancelledTask<P>) -> SinkOutcome;
}
#[non_exhaustive] enum SinkOutcome { Recorded, Requeue, Drop }
```
- Pool → **bounded mpsc** via `try_send` (full ⇒ default fallback sink; never blocks).
- **Dedicated drain task** calls `on_cancelled` under `timeout(delta)`; on overrun/error →
  **default fallback** (log + optional built-in DLQ). This is the user's "within some delta of
  time so the call doesn't hang the application."
- Registered on the pool builder (`with_cancellation_sink(Arc<dyn CancellationSink<P>>)`);
  optional — default sink logs + (for persistent backends) routes to DLQ.

### Layer C — Replay via at-least-once on persistent backends
- `SinkOutcome::Requeue` (or default for persistent queues) ⇒ **nack**: don't delete the queue
  job; release the lock / bump `attempts` / set a redelivery delay so a future consumer re-runs
  it. In-memory backend ⇒ only the caller sink provides durability.
- Document the **idempotency contract**: redelivery means executors may see a job more than
  once; callers must dedupe (e.g. on `TaskMetadata.id`).
- A `Cancelled` **lifecycle event** also fires on the CR-07 Hooks bus for metrics/observability
  (fire-and-observe; distinct from the sink which owns the replay payload).

## 4. Why a trait (and why owned `CancelledTask<P>`)
The user asked for "a trait implementation where the context of that call can be passed back."
A trait object `Arc<dyn CancellationSink<P>>` is the right extension point: testable,
composable, swappable per deployment (DB table, Kafka, file, Tauri local store). Passing the
context **by value (owned)** gives the caller everything needed to persist/replay without
borrowing pool internals — which is also what makes the decoupling sound.

## 5. Decision: R2 updated
- **R2 (revised):** **Do NOT always-drain.** Default policy stays `DrainThenCancel { deadline }`
  — inference stragglers past the deadline are **cancelled**, *because* cancelled work is now
  **captured for replay** (Layers A–C). Always-drain is rejected (hangs risk). Optional
  per-pool override remains via CR-13 `ShutdownPolicy`.

## 6. Net
Adopt the **DLQ + bounded-handoff + at-least-once nack** triad: capture an owned
`CancelledTask<P>`, hand it to a caller-supplied **`CancellationSink<P>`** off the teardown
path under a **per-call `delta` deadline** with a guaranteed fallback, and make persistent
backends **replayable via nack/redelivery** with a documented idempotency contract. Added as
**CR-14**; R2 default confirmed as cancel-stragglers-then-capture.
