//! The statements behind [`PgResultMailbox`](super::PgResultMailbox), and the
//! argument for why each is correct at its isolation level.
//!
//! # Isolation
//!
//! Statements use `READ COMMITTED`, Postgres's default. Keyed delivery holds
//! the claim's row lock in a short transaction through original-status lookup;
//! consume, peek and eviction each remain a single autocommitted statement:
//!
//! - **`consume`** is one `DELETE ... RETURNING` whose target row is chosen by
//!   a subquery that locks it with `FOR UPDATE SKIP LOCKED`. Whoever holds the
//!   row lock is the only statement that can delete the row, and the lock is
//!   taken *before* the delete, in the same statement. A second consumer that
//!   reaches the row while the first holds the lock skips it (it never waits
//!   and never sees the row twice); a second consumer whose snapshot predates
//!   the first one's commit finds the row version deleted when it tries to
//!   lock it and skips it too. So a stored entry is returned by exactly one
//!   call. The price is the same as the lease queue's: a consumer that loses
//!   the race for the only entry gets `None` instead of waiting, and with
//!   several entries under contention consumers take different ones, so strict
//!   delivery order holds per consumer, not across concurrent consumers.
//!   (Reading the row first and deleting it in a second statement would let
//!   two consumers read the same row before either deletes it. That is the
//!   `s-7` negative control.)
//! - **`deliver` with a key** first uses `INSERT ... ON CONFLICT` to claim
//!   the key and the entry insert reads the claim's `RETURNING`. The key's
//!   primary-key index serialises two claims of one key: the second waits for
//!   the first to commit, then takes the conflict path, where `DO UPDATE ...
//!   WHERE` is re-evaluated on the committed row (`READ COMMITTED`) and fails
//!   because the key is still remembered. No row is returned, so no entry is
//!   inserted: the first write won, atomically with the key. The conflicting
//!   row remains locked even when the WHERE predicate rejects the update. A
//!   second statement in that transaction reads its original status with a
//!   fresh snapshot, including a winner that committed while the claim waited.
//!   Consume never removes that fence; eviction/reuse waits for this lock.
//!   A same-statement fallback SELECT could miss that concurrent winner.
//! - **`evict_expired`** deletes expired rows by predicate. A concurrent
//!   consume that already holds a row makes the sweep wait, then skip the
//!   deleted row, so each row is counted by exactly one of them.
//!
//! # Time
//!
//! TTLs, expiry and key lifetimes use the database's `now()` (the transaction's
//! start time, or the statement's for autocommit), so clients with skewed clocks
//! agree. An entry is returned
//! strictly before `expires_at` and never at or after it, whether or not the
//! sweep has run. A key is remembered strictly before `forget_at`.
//!
//! # Negative control
//!
//! Under `--cfg parking_lot_negative_control="s-7"` consume reads the oldest
//! entry with a plain `SELECT` and deletes it with a second statement, and
//! ignores the delete's row count (scenario S-7). Test builds only.

/// Seconds to a timestamp: `NULL` seconds is a `NULL` timestamp (no TTL).
macro_rules! after {
    ($param:literal) => {
        concat!("now() + ", $param, "::float8 * interval '1 second'")
    };
}

/// Store an entry with no idempotency key.
///
/// `$1` mailbox name, `$2` mailbox key, `$3` task id, `$4` status, `$5`
/// payload or NULL, `$6` TTL seconds or NULL. Returns the new `id`.
pub(super) const DELIVER_PLAIN: &str = concat!(
    "INSERT INTO pl_result_mailbox_v3 \
         (mailbox_name, mailbox_key, task_id, status, payload, idempotency_key, expires_at) \
     VALUES ($1::text, $2::text, $3::bigint, $4::jsonb, $5::jsonb, NULL, ",
    after!("$6"),
    ") RETURNING id"
);

/// Claim an idempotency key and store the entry, or store nothing when the key
/// is still remembered.
///
/// `$1`..`$6` as [`DELIVER_PLAIN`], `$7` the idempotency key. Returns the new
/// `id`, or no row for a duplicate. `$8` is the full address (the deliberately
/// global OA-06 control substitutes an empty scope).
pub(super) const DELIVER_KEYED: &str = concat!(
    "WITH claim AS ( \
         INSERT INTO pl_result_mailbox_keys_v3 (mailbox_name, mailbox_key, idempotency_key, status, forget_at) \
         VALUES ($1::text, $8::text, $7::text, $4::jsonb, ",
    after!("$6"),
    ") ON CONFLICT (mailbox_name, mailbox_key, idempotency_key) DO UPDATE \
             SET forget_at = EXCLUDED.forget_at, status = EXCLUDED.status \
             WHERE pl_result_mailbox_keys_v3.forget_at IS NOT NULL \
               AND pl_result_mailbox_keys_v3.forget_at <= now() \
         RETURNING 1 \
     ) \
     INSERT INTO pl_result_mailbox_v3 \
         (mailbox_name, mailbox_key, task_id, status, payload, idempotency_key, expires_at) \
     SELECT $1::text, $2::text, $3::bigint, $4::jsonb, $5::jsonb, $7::text, ",
    after!("$6"),
    " FROM claim RETURNING id"
);

/// Columns every read returns.
macro_rules! entry_columns {
    () => {
        "id, task_id, status, payload, idempotency_key, \
         (floor(extract(epoch FROM delivered_at) * 1000))::bigint AS delivered_at_ms"
    };
}

/// Predicate: the row belongs to key `$1`/`$2` and has not expired.
macro_rules! live_for_key {
    () => {
        "mailbox_name = $1 AND mailbox_key = $2 AND (expires_at IS NULL OR expires_at > now())"
    };
}

/// Take and remove the oldest unexpired entry. `$1` mailbox name, `$2`
/// mailbox key.
#[cfg(not(parking_lot_negative_control = "s-7"))]
pub(super) const CONSUME: &str = concat!(
    "DELETE FROM pl_result_mailbox_v3 \
     WHERE id = ( \
         SELECT id FROM pl_result_mailbox_v3 WHERE ",
    live_for_key!(),
    " ORDER BY id LIMIT 1 FOR UPDATE SKIP LOCKED \
     ) RETURNING ",
    entry_columns!()
);

/// Delete one row by id (the second statement of the `s-7` control).
#[cfg(parking_lot_negative_control = "s-7")]
pub(super) const DELETE_BY_ID: &str = "DELETE FROM pl_result_mailbox_v3 WHERE id = $1";

/// Read the oldest unexpired entry without removing it. `$1` mailbox name,
/// `$2` mailbox key.
pub(super) const PEEK: &str = concat!(
    "SELECT ",
    entry_columns!(),
    " FROM pl_result_mailbox_v3 WHERE ",
    live_for_key!(),
    " ORDER BY id LIMIT 1"
);

/// Remove expired entries and forget expired keys of one mailbox. `$1`
/// mailbox name. Returns the number of entries removed.
pub(super) const EVICT: &str = "\
    WITH gone AS ( \
        DELETE FROM pl_result_mailbox_v3 \
        WHERE mailbox_name = $1 AND expires_at IS NOT NULL AND expires_at <= now() \
        RETURNING 1 \
    ), forgotten AS ( \
        DELETE FROM pl_result_mailbox_keys_v3 \
        WHERE mailbox_name = $1 AND forget_at IS NOT NULL AND forget_at <= now() \
        RETURNING 1 \
    ) \
    SELECT count(*) FROM gone";

/// Read original metadata while the conflicting claim's row lock is held.
/// `$1` namespace, `$2` full fence address, `$3` caller key. Missing/undecodable
/// metadata is an error, never a successful metadata-free duplicate.
pub(super) const DUPLICATE_STATUS: &str = "SELECT status FROM pl_result_mailbox_keys_v3 \
    WHERE mailbox_name = $1 AND mailbox_key = $2 AND idempotency_key = $3";
