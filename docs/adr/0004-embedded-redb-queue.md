# ADR 0004: Embedded lease queue and result mailbox on redb

## Status

Accepted (cr-28-embedded-redb-queue, phase-correctness-and-durability, milestone M2).

## Date

2026-10-22

## Context

The desktop/Tauri and single-node backend (`parking-lot-embedded`) was a JSONL file
store named after Yaque. It was not crash-safe (an append is not atomic with the
rewrite that dequeue performs; a crash mid-rewrite loses the queue), ignored priority,
removed a task before it ran (at-most-once), and implemented the synchronous
`TaskQueue`/`Mailbox` contracts that ADR 0003 retires. cr-28 replaces it with
implementations of the ADR 0003 contracts (`LeaseQueue`, `ResultMailbox`) that pass the
shared conformance suite and survive a process crash at any point.

Requirements the engine has to meet:

- every state change atomic and durable on commit (one transaction per operation);
- ordered iteration by key, to serve "highest priority first, FIFO within a priority"
  and "expired leases first" without scanning everything;
- a pure-Rust, embeddable library that builds on the MSRV (Rust 1.95), with a licence
  `deny.toml` already allows, and no server;
- crash recovery without a manual step.

## Spike: redb versus rustqueue (and others)

The task allowed a day; the facts that decide it are on crates.io and docs.rs and took
under an hour to read. Checked 2026-10-22.

| | redb | rustqueue |
|---|---|---|
| What it is | embedded ACID key-value store | embeddable job queue / broker (library and server) |
| Latest | 4.4.0, published 2026-10-13 ([crates.io](https://crates.io/crates/redb)); 4.3.0 on 2026-09-15, 4.2.0 on 2026-08-17: a release a month | 0.3.0, published 2026-05-22 ([crates.io](https://crates.io/crates/rustqueue)); 0.2.0 on 2026-03-29 |
| Downloads | about 14 million | about 10 thousand |
| MSRV | 1.90 (`rust-version`), below our 1.95 | 1.85 |
| Licence | MIT OR Apache-2.0 | MIT OR Apache-2.0 |
| Normal dependencies | `libc` only (`chrono`, `log`, `uuid` optional) | 30+: tokio, axum, tower-http, reqwest, utoipa, rust-embed, clap, config, metrics, sha2, hmac and more; and `redb ^2` (an older major) underneath |
| Last repository push | 2026-10-22 ([GitHub](https://github.com/cberner/redb)) | not checked beyond the release dates |

Findings:

- **redb** documents ACID write transactions with a single writer, copy-on-write pages
  and automatic crash recovery ("redb will automatically detect and recover from
  crashes, power loss, and other unclean shutdowns", `Database::check_integrity`
  docs, [docs.rs](https://docs.rs/redb/4.4.0/redb/struct.Database.html)). Commit is
  durable (fsync) by default (`Durability::Immediate`). Tables are ordered B-trees
  with range scans and `pop_first`; tuple keys and fixed-width tuple values are
  supported. It is the storage layer the next option itself uses.
- **rustqueue** is a job broker. Its manager API is `push`, `pull`, `ack(job_id)`,
  `fail`, `heartbeat`, plus `check_timeouts` and `detect_stalls` that the application
  has to call to expire stalled jobs
  ([docs.rs](https://docs.rs/rustqueue/0.3.0/rustqueue/engine/queue/struct.QueueManager.html)).
  An ack names a job id, not a lease: there is no fencing token, so a consumer whose
  lease expired could still ack work another consumer now holds, which ADR 0003
  forbids (`LeaseLost`). Priorities, delays, DLQ and idempotency exist but with its
  own semantics; fitting them to our contract (relative nack delay on the backend's
  clock, per-lease fence, first-write-wins mailbox kept past consume) would mean
  wrapping it and re-implementing the parts that matter, while linking an HTTP server
  stack into a desktop library. Its crash-safety rests on redb anyway.
- **Others looked at, not pursued.** `fjall` 3.1.12 (LSM tree, MIT OR Apache-2.0, MSRV
  1.90, released 2026-10-03) is mature, but a log-structured store gives no advantage
  for a queue this size and the redb transaction model maps one-to-one onto "one
  operation, one atomic commit". `sled` last released 0.34.7 on 2024-10-11. `yaque`
  0.6.6 was last released 2023-11-04. `persy` 1.8.1 (2026-06-30) is maintained, with
  about 0.76 million downloads against redb's 14 million.

## Decision

Use **redb 4.4** (`redb = "4.4"` in `[workspace.dependencies]`), implementing
`LeaseQueue` and `ResultMailbox` directly on its transactions. rustqueue is rejected.

### One operation, one write transaction

All state lives in one redb file, shared by the queue and the mailbox through
`EmbeddedStore` (redb allows one open handle per file). Every mutating operation opens
one write transaction, applies every table change, and commits once; the returned
future resolves after the commit. A crash therefore leaves the state before or after the
operation, never between (copy-on-write pages plus an atomic commit slot), and redb
recovers on the next open without our involvement.

Queue tables (`q_*`), ids are a counter that only grows, so id order is enqueue order:

| Operation | Transaction contents |
|---|---|
| `enqueue` | idempotency check on `q_idem` (hit: abort, return `Duplicate`); depth check on `q_tasks.len()`; allocate id; insert `q_tasks`, `q_state`, `q_avail`, and `q_idem`/`q_idem_rev` when keyed |
| `lease(max, d)` | move leases with `until <= now` from `q_leased`, and delays with `not_before <= now` from `q_delayed`, into `q_avail`; `pop_first` up to `max` from `q_avail` (key `(255 - priority, id)`: highest priority first, then FIFO, an expired or requeued entry keeps its place); for each: attempts + 1, new fence from the counter, insert `q_leased`, update `q_state` |
| `ack` | check the token (`q_state` row is `Leased`, fence equal, `now < until`; else `LeaseLost` and abort); remove `q_leased`, `q_state`, `q_tasks`, idempotency key |
| `nack` requeue | check token; remove `q_leased`; zero delay: back into `q_avail`; else insert `q_delayed (now + delay)`; update `q_state` |
| `nack` dead-letter | check token; move the task into `q_dead` with attempts and reason; remove from the live tables; free the key |
| `extend` | check token; move the `q_leased` key to `now + d`; update `q_state` |
| `depth` | one read transaction (a consistent snapshot) |

Mailbox tables (`mb_*`): `deliver` checks `mb_idem` (a remembered, unexpired key:
abort, `Duplicate`), then inserts the entry under `(mailbox key JSON, sequence)` and the
key with its forget-at time in one transaction. `consume` reads the oldest unexpired
entry of a key and removes it (and any expired ones it passed) in one write
transaction, so two consumers cannot both receive it and a result never vanishes
without having been returned; the idempotency key stays until its TTL passes.
`evict_expired` removes expired entries and keys in one transaction.

### Time

Leases, delays and TTLs are stored as wall-clock milliseconds taken from the injected
`Clock::now_ms`, because they must keep their meaning after a restart (the monotonic
clock restarts at zero). Durations round up to a whole millisecond. Consequence: a wall
clock that steps backwards extends the leases it covers by the size of the step; a step
forward expires them early and causes a redelivery, which at-least-once delivery
already tolerates.

### Async

redb's API is blocking and its commit fsyncs, so each operation runs on tokio's
blocking pool (`tokio::task::spawn_blocking`); this crate depends on tokio's `rt`
feature only. A dropped future does not abandon a transaction: the blocking task runs
to its end, so the operation either applied or did not, matching the cancellation notes
on the contracts.

### Crash tests

`crates/parking-lot-embedded/tests/crash_points.rs` runs the real store, queue and
mailbox in a child process (the test binary re-invoked) and kills it with SIGKILL at
every named point: before and after the commit of `enqueue`, `lease`, `deliver`, `ack`,
`consume`, `extend` and `nack` (feature `fault-injection` parks the child inside the
operation), and between every pair of consecutive workload steps. The parent reopens
the file and checks that no enqueued, un-acked task is lost, nothing acked returns,
redelivered tasks have a higher attempt, and each stored result is consumable exactly
once and survives a repeated delivery as `Duplicate`. SIGKILL gives the same file state
as a panic-abort (the OS keeps what the process wrote); it does not model power loss,
which is redb's fsync contract. The negative control under
`--cfg parking_lot_negative_control="s-8"` acknowledges an enqueue without committing
it and the tests report `ACCEPTED TASK LOST`.

## Consequences

- `YaqueQueue`, `YaqueMailbox` and their JSONL files are removed; there is no migration
  (the old files were never crash-safe, and no released version documented them as a
  storage format). The facade's `infra::{queue,mailbox}` re-exports now name
  `RedbLeaseQueue`, `RedbResultMailbox` and `EmbeddedStore`.
- The embedded crate no longer implements the synchronous `TaskQueue`/`Mailbox`. The
  pool cannot use it until cr-49 moves `ResourcePool` onto the async contracts; until
  then it is usable directly (see `examples/embedded_pool.rs`).
- `lease` is O(k log n) in the entries it moves and takes; `depth` and `evict_expired`
  scan the lease, delay and mailbox tables (O(n)), which is acceptable at desktop
  scale and noted for revisiting if a profile says otherwise.
- redb holds an exclusive file lock: one process per database file. A second open fails
  with a backend error rather than corrupting anything.
- A task whose stored JSON can no longer be decoded makes `lease` fail with a backend
  error instead of skipping it; schema evolution of payloads is the caller's concern,
  and the on-disk layout carries a version (`schema_version`, currently 1) that `open`
  checks.

## Alternatives considered

- **rustqueue** and **fjall**: see the spike table.
- **SQLite (rusqlite)**: mature and ACID, but a C dependency and a build step in a crate
  whose point is a light embedded store; redb is pure Rust.
- **Keeping the JSONL format and making it atomic** (write-temp-then-rename, fsync):
  would fix the rewrite but not priority, leases or fencing, and the whole file is
  rewritten per operation.
- **A cross-restart monotonic clock**: none exists portably; wall time with the stated
  caveat is the honest choice.
