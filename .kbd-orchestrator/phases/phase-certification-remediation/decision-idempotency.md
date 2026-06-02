# Decision Record — Idempotency & replay semantics

**Context:** CR-14 introduces at-least-once redelivery of cancelled tasks. At-least-once
*guarantees* duplicates, which forces explicit idempotency decisions. Locked with the user
2026-06-02. **Informs:** CR-14, CR-05 (error/metadata), CR-06 (mailbox), CR-09 (docs).

> ⚠️ Foundational code gap: `WorkerPool::submit()` generates an internal sequential
> `task_id_counter` and mailbox key and **ignores the caller's `meta.id`** (native.rs:315-316).
> There is currently **no** idempotency key and **no** attempts/dedup wiring (`attempts` exists
> only in the Postgres schema doc). CR-14 must close this.

## Locked decisions

### D1 — Idempotency key = explicit `idempotency_key` field ✅
- Add `idempotency_key: Option<String>` to `TaskMetadata` (distinct from internal routing id).
- Caller owns logical identity. `None` ⇒ task is treated as unique (no dedup, no replay match).
- Routing/result key derives from `idempotency_key` when present; internal id remains for
  pool-internal bookkeeping only.

### D2 — Dedup ownership = idempotent executor; library supplies the key ✅
- Library guarantees **at-least-once** and provides the stable key. It does **not** attempt
  exactly-once.
- The **executor/caller must be idempotent** (e.g. keyed result cache, dedupe on the key).
- (Deferred, opt-in only if later requested) a best-effort TTL seen-key cache as a convenience
  layer — not in v1.

### D3 — Replay scope by `CancelReason` = env-only auto-requeue ✅
| `CancelReason` | Action |
|----------------|--------|
| `QueueShutdown`, `DrainTimeout` | **auto-requeue** (environmental; not the task's fault) |
| `DeadlineExpired` | **DLQ only** (may hang again — never auto-loop) |
| `SignalCancelled` | **DLQ only** (caller cancelled deliberately) |
- DLQ entries are replayed **deliberately by the caller**, never automatically.

### D4 — Retry bound = `max_attempts = 3`, exponential backoff, both configurable ✅
- Default 3 attempts; exponential redelivery delay; on exhaustion ⇒ **terminal DLQ**.
- `max_attempts` and backoff base are per-pool config.

## Adopted defaults (no user input needed)

### D5 — Result/mailbox idempotency = first-write-wins + consume-once
- Results keyed by `idempotency_key`. If a duplicate execution produces a second result for an
  already-populated key, **ignore the later write** (first-write-wins). Consume-once on retrieve.

### D6 — Partial side-effects on replay = documented caller contract (out of library scope)
- A cancelled inference may have already streamed tokens / made external calls; replay redoes
  them. Library cannot prevent this. The `CancelledTask<P>` context is handed back so the caller
  can choose **resume vs restart**. Document the contract; do not attempt checkpointing in v1.

### D7 — Cross-restart persistence = key + attempts in persisted metadata
- `idempotency_key` and `attempt` persist in the queue row metadata (JSONB exists) so replay
  survives restarts. **In-memory backend = no cross-restart guarantee** (documented limitation).

## Net contract (for docs / CR-09)
> Delivery is **at-least-once**. Set `TaskMetadata.idempotency_key` for any task you want
> deduplicated or replayable, and make your executor idempotent on that key. Shutdown/drain
> cancellations auto-requeue (≤ `max_attempts`, exponential backoff); deadline-expiry and
> explicit cancels go to the dead-letter store for deliberate caller replay. Persistent backends
> survive restarts; the in-memory backend does not.
