# ADR 0003: Async backend contracts — lease queue, result mailbox, notifier, conformance suite

## Status

Accepted (cr-25-backend-contracts, phase-correctness-and-durability, milestone M1).

## Date

2026-10-22

## Context

The 0.2 storage contracts are synchronous: `TaskQueue::enqueue/dequeue` and
`Mailbox::deliver/consume` take `&mut self`. The Postgres and Yaque backends implement
them by calling `block_on` inside the async runtime (which panics on a current-thread
runtime and stalls a worker thread on a multi-threaded one), and `dequeue` removes the
task before it runs, so a crash between dequeue and completion loses accepted work: the
queue is at-most-once. There is no notifier type, although cr-32 and cr-34 need one.

The dependents need, from one set of contracts:

| Change | Needs |
|---|---|
| cr-26 Postgres lease queue, cr-30 pgmq | an async queue with lease, ack, nack (requeue with backoff, dead-letter), extend, and a shared test suite to pass |
| cr-27 Postgres mailbox | async deliver with first-write-wins on an idempotency key, consume-once, TTL eviction |
| cr-28 embedded redb queue | the same queue and mailbox contracts, crash-safe |
| cr-34 mailbox composition | a `Notifier` and the `StorageMailbox<S, N>` type |
| cr-49 durable dispatch | persist before acknowledging acceptance; dispatch from `lease`; write the mailbox, then ack; heartbeat with `extend`; redelivery of unacked work after a crash |
| cr-37 config builder, cr-44 bindings | backends nameable as types, selectable from config |

## Decision

### New traits beside the synchronous ones

Two async traits in `prometheus_parking_lot_core::contracts` (re-exported at the crate
root and by the facade under `core::backend`):

- `LeaseQueue<P: Send>: Send + Sync` — `enqueue`, `lease`, `ack`, `nack`, `extend`,
  `depth`, `max_depth`, and `identity` (added by the amendment below).
- `ResultMailbox<T: Send>: Send + Sync` — `deliver`, `consume`, `peek`, `evict_expired`.

They take `&self` (backends hold their own pools or locks), so one instance is shared
between submitters, dispatchers and heartbeats.

The synchronous `TaskQueue` and `Mailbox` stay, unchanged, because `ResourcePool`
(runtime-tokio, scheduler-engineer) still parks tasks in an `InMemoryQueue` and delivers
through `Mailbox` under its locks, and the Postgres/Yaque backends still implement them.
They are retired by the changes that replace their users: cr-26/27/28 drop the
synchronous backend implementations, and cr-49 moves the pool onto `LeaseQueue` and
`ResultMailbox` (the in-process parking heap can stay a concrete type). The new traits
get distinct names rather than reusing `TaskQueue`/`Mailbox` so that, while both exist,
no implementer can pick the wrong one by import path, and no file owned by another role
had to change in this change.

### `impl Future + Send` in the trait, not `async-trait`

Every method is declared `fn m(&self, ..) -> impl Future<Output = ..> + Send`.
Implementations write plain `async fn` (or return a ready future); the declared `+ Send`
makes the compiler check each implementation's future at the impl site, so generic
dispatch code can `tokio::spawn` (or any multi-threaded spawner) without naming the
future type. No allocation per call, no macro, and the core stays runtime-free.

Send story:

- Traits require `Send + Sync`; futures are `Send`. A backend whose IO futures are not
  `Send` (browser storage through `wasm-bindgen`) cannot implement them directly. On
  `wasm32` the in-memory backends are used; if a browser-storage backend is ever needed,
  a `?Send` sibling trait is added then, not now.
- Payloads must be `Send`; the in-memory backends additionally need `P: Clone` (a
  leased copy is handed out while the entry is kept for redelivery). Durable backends
  need `Serialize + DeserializeOwned` instead, stated on their impls.

Consequence: the traits are not dyn-compatible. Runtime selection from config (cr-37)
uses an enum over the compiled-in backends, or an erased `DynLeaseQueue`/
`DynResultMailbox` adapter with boxed futures, which the tech-lead adds in cr-37 if the
builder needs it. `TaskExecutor` keeps `async-trait`; it is not part of this decision.

### Lease semantics

- **Durable acceptance.** `enqueue` resolves `Ok(Enqueued::Accepted(id))` only once the
  task is stored; a pool acknowledges submission after that (cr-49). Backends assign a
  `QueueEntryId` (the pool's `TaskMetadata::id` is per process and not unique across
  restarts).
- **Idempotent enqueue.** If the task's `idempotency_key` matches an entry still in the
  queue (ready, delayed or leased), nothing is stored and `Enqueued::Duplicate(id)` is
  returned. After ack or dead-letter the key is free again. This makes "retry an enqueue
  whose outcome is unknown" safe (Postgres: a partial unique index plus
  `ON CONFLICT DO NOTHING`).
- **Leases and fencing.** `lease(max, lease_for)` returns up to `max` ready tasks as
  `LeasedTask { task, token, attempt }` and returns immediately (empty when nothing is
  ready). A leased task is invisible until acked, nacked or expired. `LeaseToken` is
  `(entry, fence)`; every lease gets a new fence. `ack`, `nack` and `extend` succeed only
  for the entry's current, *unexpired* lease and otherwise return the new
  `SchedulerError::LeaseLost { entry }` (not retryable) — including after expiry but
  before redelivery, so a slow consumer can never ack work that may already belong to
  someone else. Tokens are `Copy + Serialize` so a heartbeat task can hold them.
- **Attempts.** `attempt` is 1 on first delivery and increases on every redelivery
  (nack-requeue or lease expiry). Poison handling (dead-letter after N attempts) is
  dispatcher policy (cr-49), not queue behaviour.
- **Nack.** `NackAction::Requeue { delay }` makes the task ready again after `delay`
  (backoff; a relative duration so it means the same on every backend's clock) and keeps
  its place; `NackAction::DeadLetter { reason }` removes it from the live set for good.
  Dead letters count in `QueueDepth::dead_lettered`, not toward `max_depth`. Inspecting
  and replaying dead letters is not part of the contract yet (the in-memory backend
  exposes `dead_letters()`).
- **Extend** sets the expiry to `now + lease_for` (not old expiry + `lease_for`); an
  expired lease cannot be revived.
- **Ordering.** Highest priority first, then enqueue order; requeued and expired tasks
  keep their original place. Backends order by their own monotonic column (ADR 0002).
- **Deadlines** are not enforced by the queue: an expired task is still leased, and the
  dispatcher delivers `Expired` to the mailbox and acks it.
- **Time.** Leases and delays run on the backend's clock: `Clock::monotonic` for the
  in-memory backend, database time for Postgres (consistent across processes).
- **Cancellation.** Each method documents what a dropped future leaves behind. No
  cancellation loses an accepted task; at worst a lease is held by nobody until it
  expires, or an enqueue's outcome is unknown (retry with an idempotency key).
- **Waiting for work** is outside the contract: `lease` never blocks. The cr-49
  dispatcher polls with backoff and is woken in-process by its own submissions; a
  cross-process wake-up (Postgres `LISTEN/NOTIFY`, cr-26) is an additive contract
  addition when cr-26/cr-49 settle its shape.

### Mailbox semantics

- A mailbox holds zero or more `MailboxEntry { task_id, status, payload,
  idempotency_key, delivered_at_ms }` per `MailboxKey`, in delivery order.
  `consume(key)` takes the oldest unexpired entry (consume-once); `peek` reads it.
- **First write wins**, and the idempotency key is remembered **past consume** until the
  entry's TTL evicts it (forever without a TTL). Otherwise a task redelivered after the
  client already consumed its result (crash between mailbox write and ack) would hand the
  client a second result. Backends keep a tombstone or a separate key table; key
  retention must outlive the queue's longest redelivery path.
- `deliver` returns `DeliveryOutcome::{Stored, Duplicate}`; a different payload under a
  remembered key is still `Duplicate` (re-executions of nondeterministic work are
  expected to differ; the first result stands).
- **TTL** is relative and starts at storage. Expired entries are never returned;
  `evict_expired` removes them and forgets their keys.
- `TaskStatus` must round-trip in full, including `Failed(TaskError)`; the conformance
  suite checks it.

### Notifier and composition

`Notifier<T: Sync>: Send + Sync` with `notify(&key, &entry) -> Result<(), SchedulerError>`
(best effort), plus `NoopNotifier`. `StorageMailbox<S, N = NoopNotifier>` is defined here
as a type only (`new`, `storage_only`, accessors, `into_parts`); its `ResultMailbox`
implementation — store first, notify only after `Stored`, never fail or undo the stored
delivery on notifier failure, bounded retries — is cr-34 (events-engineer, `mailbox`
module). Off-path notification and retry backoff need a spawner and a timer; if cr-34
needs extra fields or type parameters for them, that is a contract handoff to the
tech-lead.

### Conformance suite

Feature `conformance` on `prometheus-parking-lot-core` (no extra dependencies) publishes
`contracts::conformance`:

- `QueueHarness` / `MailboxHarness`: build a fresh, isolated backend per case and
  `advance` its clock (a `TestClock` advance, or a real sleep for database time with a
  lowered `lease_duration()`/`ttl()`).
- 14 queue cases (round trip of metadata and payload, invisibility, expiry redelivery,
  fencing after expiry, ack, nack requeue, delayed requeue, dead-letter, extend,
  priority-then-FIFO across lease calls, requeued task keeps its place, argument
  validation, max depth counts live entries only, idempotent enqueue) and 8 mailbox
  cases (consume-once with full status round trip, peek, per-key order, key isolation,
  first-write-wins across keys, idempotency survives consume, unkeyed deliveries, TTL
  hide/evict/forget).
- `lease_queue_conformance!` / `result_mailbox_conformance!` generate one test per case
  (`test_attr = #[tokio::test]` or `runner = futures::executor::block_on`);
  `queue::run_all` / `mailbox::run_all` run all cases in one test.

The in-memory reference backends (`InMemoryLeaseQueue`, `InMemoryResultMailbox`, both
generic over a `Clock`) run the suite in the core crate's unit tests, together with two
negative controls (a queue without fencing, a mailbox without first-write-wins) that the
suite must reject.

## Consequences

- Additive for 0.3.0: new traits, types and the `LeaseLost` error variant
  (`SchedulerError` is `#[non_exhaustive]`). Nothing existing changed.
- cr-26, cr-27, cr-28 and cr-30 implement `LeaseQueue`/`ResultMailbox` and run the suite
  with `features = ["conformance"]` under `[dev-dependencies]` (manifest change through
  the tech-lead).
- cr-49 builds durable dispatch on: `enqueue` before acknowledging submission; `lease`
  as the dispatcher's source; `deliver` with the task's idempotency key, then `ack`;
  `extend` as heartbeat; `nack` for retries and poison tasks; and then retires the
  synchronous `TaskQueue`/`Mailbox`.
- Two generations of contracts coexist until cr-49; the module docs say which is which.

## Alternatives considered

- **`async-trait`.** Dyn-compatible for free, but boxes every future and hides the
  `Send` bound in a macro. Rejected: the hot path (lease/ack per task) pays an
  allocation, and dyn dispatch is needed only at the config boundary, where an enum or
  an erased adapter serves.
- **Native `async fn` in the trait without `+ Send`.** Callers could not spawn the
  futures on multi-threaded runtimes without return-type notation (not stable).
- **Rename the synchronous traits now and take their names.** Rejected for this change:
  it edits `ResourcePool`, the Postgres and Yaque backends and their tests (three other
  owners, one mid-change) for no behavioural gain, since cr-26/27/28/49 replace those
  users anyway.
- **Blocking `lease` with a wait timeout.** Needs a timer in the runtime-free core and
  would hide the wake-up mechanism differences between backends; deferred as above.
- **Absolute `not_before` on requeue.** Clocks differ between the dispatcher and a
  database backend; a relative delay means the same everywhere.
- **One mailbox entry per key (0.2 in-memory behaviour).** A later delivery overwrote an
  unconsumed result for the same session; rejected as silent result loss.
- **Forget idempotency keys on consume.** Simpler, but loses the protection exactly in
  the crash-between-deliver-and-ack case durable dispatch must survive.

## Amendment (2026-10-23): queue identity

The 0.3.0 gate review found that a durable pool keyed results it delivered without a
task idempotency key as `"<pool name>:entry:<entry id>"`. Entry ids are unique only
within one queue, and pool names are chosen by operators, so exactly-one-result
depended on naming: differently named consumers of one queue stored a second result
after a crash between delivery and ack, and pools on two queues sharing a mailbox
dropped each other's results.

Decision: `LeaseQueue` gains a required `fn identity(&self) -> &str`, a stable name for
the queue that qualifies its entry ids (the same for every handle, in every process and
across restarts; different for queues whose ids could repeat). Durable pools key
results `"<queue identity>:entry:<entry id>"`. Required rather than defaulted, so no
backend or wrapper silently inherits a shared default and reintroduces the collision.

Known limit: a queue dropped and recreated under the same identity (a Postgres table or
pgmq queue whose id sequence restarts) can reuse entry ids. Rejected alternative: a
per-entry random token stored with the task, which needs a schema change in every
backend; revisit if recreated queues prove common.
