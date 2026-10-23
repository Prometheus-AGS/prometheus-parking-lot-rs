//! The statements behind [`PgLeaseQueue`](super::PgLeaseQueue), and the
//! argument for why each is correct at its isolation level.
//!
//! # Isolation
//!
//! Every statement runs at `READ COMMITTED`, Postgres's default, as a single
//! autocommitted statement. Enqueue is the one exception: a short transaction
//! of four statements. Nothing needs a stronger level, because each statement
//! that changes an entry either takes the row lock itself or re-checks its
//! `WHERE` clause after waiting for it:
//!
//! - **`lease`** selects candidates with `FOR UPDATE SKIP LOCKED`, so a row
//!   another transaction holds is skipped, not waited for. A row that another
//!   transaction changed and committed after this statement's snapshot is
//!   re-checked against the `WHERE` clause once locked (`READ COMMITTED`
//!   re-evaluates the qualification on the newest row version); if it no longer
//!   qualifies it is dropped. So two leases never return the same entry at the
//!   same time. The price is that a lease can return fewer than `max` entries
//!   under contention; the next call (or the wake-up) finds the rest.
//! - **`ack`, `nack`, `extend`** are single `UPDATE`/`DELETE` statements
//!   matching `(id, state = 'leased', attempts = fence, lease not expired)`.
//!   A concurrent `lease` re-claiming an expired entry also takes the row lock;
//!   whichever statement gets the lock second re-checks its `WHERE`. If the
//!   re-claim won, `attempts` changed and the stale token matches nothing, which
//!   is reported as `LeaseLost`. If the ack won, the entry is gone and the
//!   re-claim finds nothing. Fencing needs no extra read.
//! - **`reap`** is one `UPDATE` over expired leases, whose rows are picked by
//!   an `ORDER BY id ... FOR UPDATE SKIP LOCKED` sub-select. It conflicts with
//!   `extend` only on the row lock, and `extend` requires the lease unexpired
//!   while `reap` requires it expired, so a row qualifies for at most one of
//!   them. A row some other statement holds is left for the next pass, so the
//!   reaper never waits.
//! - **The `lease` statement's attempts-exhausted sweep** picks its rows the
//!   same way (`ORDER BY id ... FOR UPDATE SKIP LOCKED`), so neither half of
//!   `lease` ever waits for a row lock. Without it, a `lease` and a reaper pass
//!   locking the same expired rows in different scan orders could wait on each
//!   other (deadlock), and a lease would wait behind any open transaction that
//!   touched such a row.
//! - **`enqueue`** takes a per-queue transaction-scoped advisory lock, then
//!   checks the idempotency key and the live count and inserts. The lock makes
//!   the check-then-insert atomic among enqueuers of the same queue, so
//!   `max_depth` is exact and a key is stored once. Each statement in the
//!   transaction gets a fresh snapshot, so the count sees every enqueue that
//!   committed before the lock was granted. Leases and acks do not take the
//!   advisory lock; a concurrent ack can only lower the count, so the check
//!   errs towards rejecting, never towards overfilling.
//!
//! # Time
//!
//! All times are the database's `now()` (the statement's start time), so lease
//! expiry does not depend on any client clock and clients with skewed clocks
//! agree. A lease is valid strictly before `lease_expires_at`.
//!
//! # Negative controls
//!
//! Under `--cfg parking_lot_negative_control="s-5"` leases never expire
//! (scenario S-5); under `"s-6"` the `FOR UPDATE SKIP LOCKED` clause is
//! omitted (scenario S-6); under `"s-5-fencing"` the fence comparison is
//! dropped from ack, nack and extend. Test builds only.

use std::sync::LazyLock;

/// Name of the table (created by `migrations/004_lease_queue.sql`).
pub(super) const TABLE: &str = "pl_lease_queue";

/// The `LISTEN`/`NOTIFY` channel the table's triggers notify on; the payload is
/// the queue name.
pub const NOTIFY_CHANNEL: &str = "pl_lease_queue";

/// The migration that creates the table, indexes and triggers.
pub(super) const MIGRATION: &str = include_str!("../../migrations/004_lease_queue.sql");

/// SQL boolean: the lease on this row has expired.
#[cfg(not(parking_lot_negative_control = "s-5"))]
const EXPIRED: &str = "lease_expires_at <= now()";
#[cfg(parking_lot_negative_control = "s-5")]
const EXPIRED: &str = "false";

/// SQL boolean: the lease on this row is still valid.
#[cfg(not(parking_lot_negative_control = "s-5"))]
const LIVE: &str = "lease_expires_at > now()";
#[cfg(parking_lot_negative_control = "s-5")]
const LIVE: &str = "true";

/// Row-locking clause of the lease candidate scan.
#[cfg(not(parking_lot_negative_control = "s-6"))]
const ROW_LOCK: &str = "FOR UPDATE SKIP LOCKED";
#[cfg(parking_lot_negative_control = "s-6")]
const ROW_LOCK: &str = "";

/// Fence comparison of ack, nack and extend (`$3` is the token's fence).
#[cfg(not(parking_lot_negative_control = "s-5-fencing"))]
const FENCE: &str = "attempts = $3";
#[cfg(parking_lot_negative_control = "s-5-fencing")]
const FENCE: &str = "$3::int4 = $3::int4";

/// The statements, built once from the constants above and leaked so they are
/// `&'static str` (sqlx 0.9 only accepts static SQL without an explicit
/// `AssertSqlSafe`). None contains caller data; every value is a bind parameter.
pub(super) struct Statements {
    /// `$1` queue, `$2` max entries, `$3` lease seconds, `$4` max attempts or
    /// NULL. Returns `id, priority, meta, payload, attempts`.
    pub lease: &'static str,
    /// `$1` queue, `$2` id, `$3` fence.
    pub ack: &'static str,
    /// `$1` queue, `$2` id, `$3` fence, `$4` delay seconds.
    pub nack_requeue: &'static str,
    /// `$1` queue, `$2` id, `$3` fence, `$4` reason.
    pub nack_dead_letter: &'static str,
    /// `$1` queue, `$2` id, `$3` fence, `$4` lease seconds.
    pub extend: &'static str,
    /// `$1` queue, `$2` max attempts or NULL. Returns `(requeued, dead)`.
    pub reap: &'static str,
    /// `$1` queue. Returns `(ready, delayed, leased, dead)`.
    pub depth: &'static str,
}

/// An entry is leasable when it is ready and its backoff has passed, or its
/// lease has expired.
fn leasable() -> String {
    format!("((state = 'ready' AND not_before <= now()) OR (state = 'leased' AND {EXPIRED}))")
}

/// The statements, with the negative-control clauses of this build applied.
pub(super) static STATEMENTS: LazyLock<Statements> = LazyLock::new(|| {
    let leasable = leasable();
    let held = format!("queue_name = $1 AND id = $2 AND state = 'leased' AND {FENCE} AND {LIVE}");

    // `exhausted` dead-letters leasable entries that already used up their
    // attempts (a task that crashes its consumer never reaches a nack); `next`
    // picks what to lease from the rest. The two row sets are disjoint, so one
    // statement can do both.
    let lease = format!(
        "WITH exhausted AS (
            UPDATE {TABLE}
               SET state = 'dead', lease_expires_at = NULL,
                   dead_reason = 'max attempts (' || $4::int || ') exceeded'
             WHERE id IN (
                   SELECT id FROM {TABLE}
                    WHERE queue_name = $1 AND $4::int IS NOT NULL AND attempts >= $4::int
                      AND {leasable}
                    ORDER BY id
                    {ROW_LOCK})
         ), next AS (
            SELECT id FROM {TABLE}
             WHERE queue_name = $1
               AND ($4::int IS NULL OR attempts < $4::int)
               AND {leasable}
             ORDER BY priority DESC, id ASC
             LIMIT $2
             {ROW_LOCK}
         )
         UPDATE {TABLE} q
            SET state = 'leased', attempts = q.attempts + 1,
                lease_expires_at = now() + make_interval(secs => $3)
           FROM next
          WHERE q.id = next.id
      RETURNING q.id, q.priority, q.meta, q.payload, q.attempts"
    );
    let ack = format!("DELETE FROM {TABLE} WHERE {held}");
    let nack_requeue = format!(
        "UPDATE {TABLE}
            SET state = 'ready', lease_expires_at = NULL,
                not_before = now() + make_interval(secs => $4)
          WHERE {held}"
    );
    let nack_dead_letter = format!(
        "UPDATE {TABLE}
            SET state = 'dead', lease_expires_at = NULL, dead_reason = $4
          WHERE {held}"
    );
    let extend = format!(
        "UPDATE {TABLE}
            SET lease_expires_at = now() + make_interval(secs => $4)
          WHERE {held}"
    );
    let reap = format!(
        "WITH reaped AS (
            UPDATE {TABLE}
               SET state = CASE WHEN $2::int IS NOT NULL AND attempts >= $2::int
                                THEN 'dead' ELSE 'ready' END,
                   dead_reason = CASE WHEN $2::int IS NOT NULL AND attempts >= $2::int
                                      THEN 'max attempts (' || $2::int || ') exceeded' END,
                   lease_expires_at = NULL
             WHERE id IN (
                   SELECT id FROM {TABLE}
                    WHERE queue_name = $1 AND state = 'leased' AND {EXPIRED}
                    ORDER BY id
                    {ROW_LOCK})
         RETURNING state
         )
         SELECT count(*) FILTER (WHERE state = 'ready'),
                count(*) FILTER (WHERE state = 'dead')
           FROM reaped"
    );
    let depth = format!(
        "SELECT count(*) FILTER (WHERE {leasable}),
                count(*) FILTER (WHERE state = 'ready' AND not_before > now()),
                count(*) FILTER (WHERE state = 'leased' AND {LIVE}),
                count(*) FILTER (WHERE state = 'dead')
           FROM {TABLE} WHERE queue_name = $1"
    );

    Statements {
        lease: lease.leak(),
        ack: ack.leak(),
        nack_requeue: nack_requeue.leak(),
        nack_dead_letter: nack_dead_letter.leak(),
        extend: extend.leak(),
        reap: reap.leak(),
        depth: depth.leak(),
    }
});

/// Serialise enqueuers of one queue for the rest of the transaction.
/// `$1` queue.
pub(super) const ENQUEUE_LOCK: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('pl_lease_queue:' || $1, 0))";

/// The non-dead entry holding an idempotency key. `$1` queue, `$2` key.
pub(super) const ENQUEUE_EXISTING: &str = "SELECT id FROM pl_lease_queue
     WHERE queue_name = $1 AND idempotency_key = $2 AND state <> 'dead'";

/// Live entries, counted up to the limit. `$1` queue, `$2` limit.
pub(super) const ENQUEUE_COUNT: &str = "SELECT count(*) FROM
     (SELECT 1 FROM pl_lease_queue WHERE queue_name = $1 AND state <> 'dead' LIMIT $2) live";

/// Store the entry. `$1` queue, `$2` priority, `$3` key, `$4` meta, `$5` payload.
pub(super) const ENQUEUE_INSERT: &str = "INSERT INTO pl_lease_queue
     (queue_name, priority, idempotency_key, meta, payload)
     VALUES ($1, $2, $3, $4, $5) RETURNING id";

/// Dead-letter an entry whose stored task cannot be decoded. `$1` queue,
/// `$2` id, `$3` fence, `$4` reason.
pub(super) const DEAD_LETTER_UNDECODABLE: &str = "UPDATE pl_lease_queue
     SET state = 'dead', lease_expires_at = NULL, dead_reason = $4
     WHERE queue_name = $1 AND id = $2 AND state = 'leased' AND attempts = $3";
