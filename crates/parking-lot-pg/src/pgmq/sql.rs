//! The statements behind [`PgmqLeaseQueue`](super::PgmqLeaseQueue), and the
//! argument for why each is correct.
//!
//! # pgmq functions and the two reads of pgmq's tables
//!
//! Every state change goes through a pgmq function: `pgmq.send`, `pgmq.read`,
//! `pgmq.set_vt` and `pgmq.delete` (pgmq 1.13.1; signatures verified against
//! `pgmq--1.13.1.sql`). pgmq's functions do not check who is calling, so the
//! fence cannot be expressed with them alone: `pgmq.delete(queue, msg_id)`
//! deletes whatever holds that id. Two statement kinds therefore also read
//! pgmq's documented queue table `pgmq.q_<queue>`:
//!
//! - the **fence guard** (`held`), which locks the message row
//!   `FOR UPDATE` only if its `read_ct` equals the token's fence and its
//!   visibility timeout has not passed, and
//! - **depth**, which counts rows by `vt`.
//!
//! The pgmq function runs on the guarded row in the same statement, so the
//! check and the change are one atomic step at `READ COMMITTED`: a concurrent
//! `pgmq.read` that re-claims an expired message also locks the row, and
//! whichever statement gets the lock second re-evaluates `read_ct` and `vt`
//! against the new row version. The loser is a `LeaseLost`, never a double
//! claim.
//!
//! The delay mark of a nacked token lives in another table, so the lock on the
//! message row alone does not cover it. A statement that waited for the row
//! lock re-checks the new row version with its *old* snapshot, which cannot see
//! a nack that committed while it waited. That matters because dropping a Rust
//! future does not cancel its SQL statement: a heartbeat `extend` can still be
//! queued behind a `nack(Requeue { delay > 0 })` and would otherwise pass,
//! set `vt = now + lease`, and make the nacked token valid again once the delay
//! has elapsed. The guard therefore calls `pl_pgmq_is_delayed` (migration 006),
//! a volatile plpgsql function whose queries take a fresh snapshot each time,
//! from the same `WHERE` clause that locks the row: the re-check after the wait
//! runs it again and sees the nack. This fences any two statements racing on
//! one token (`extend` vs `nack`, `ack` vs `nack`, ...); the loser is a
//! `LeaseLost`.
//!
//! # Lease predicate
//!
//! A lease is held while `vt > clock_timestamp()`, the exact negation of the
//! predicate `pgmq.read` uses to hand a message out (`vt <= clock_timestamp()`).
//! A token is therefore valid if and only if no other `lease` call could
//! receive that message right now.
//!
//! # Identifiers
//!
//! Table and queue names are built from the logical queue name, which
//! [`PgmqLeaseQueue::new`](super::PgmqLeaseQueue::new) restricts to
//! `[a-z0-9_]`, so the statements can be assembled once with `format!` and
//! contain no caller data. Every value is a bind parameter.
//!
//! # Negative control
//!
//! Under `--cfg parking_lot_negative_control="s-18"` every priority maps to
//! lane 0: one pgmq queue, no priority split (scenario S-18). Test builds only.

use sqlx::{AssertSqlSafe, SqlSafeStr, SqlStr};

/// Number of priority lanes (`Priority` has four levels).
pub(super) const LANES: usize = 4;

/// The migration (extension, side table).
pub(super) const MIGRATION: &str = include_str!("../../migrations/006_pgmq_lease_queue.sql");

/// Lane a priority level (0 = low .. 3 = critical) is stored in.
#[cfg(not(parking_lot_negative_control = "s-18"))]
pub(super) const fn lane_of(level: usize) -> usize {
    level
}
#[cfg(parking_lot_negative_control = "s-18")]
pub(super) const fn lane_of(_level: usize) -> usize {
    0
}

/// Serialise enqueuers of one queue for the rest of the transaction.
/// `$1` logical queue.
pub(super) const ENQUEUE_LOCK: &str =
    "SELECT pg_advisory_xact_lock(hashtextextended('pl_pgmq_queue:' || $1, 0))";

/// The entry holding an idempotency key. `$1` logical queue, `$2` key.
pub(super) const ENQUEUE_EXISTING: &str =
    "SELECT entry_id FROM pl_pgmq_aux WHERE queue_name = $1 AND idem_key = $2";

/// Record a key. `$1` logical queue, `$2` entry id, `$3` key.
pub(super) const ENQUEUE_KEY: &str =
    "INSERT INTO pl_pgmq_aux (queue_name, entry_id, idem_key) VALUES ($1, $2, $3)";

/// Store the task. `$1` pgmq queue, `$2` message.
pub(super) const ENQUEUE_SEND: &str = "SELECT pgmq.send($1::text, $2::jsonb)";

/// Wake idle dispatchers (delivered at commit). The channel is
/// [`NOTIFY_CHANNEL`](crate::NOTIFY_CHANNEL), shared with
/// [`PgLeaseQueue`](crate::PgLeaseQueue); the payload is the logical queue name.
/// `$1` logical queue.
pub(super) const ENQUEUE_NOTIFY: &str = "SELECT pg_notify('pl_lease_queue', $1)";

/// Lease from one lane. `$1` pgmq queue, `$2` visibility timeout (seconds),
/// `$3` quantity. Returns `msg_id, read_ct, message`.
pub(super) const LEASE_READ: &str =
    "SELECT msg_id, read_ct, message FROM pgmq.read($1::text, $2::int, $3::int)";

/// Create one pgmq queue (idempotent). `$1` queue.
///
/// `pgmq.create` on an existing queue runs `CREATE INDEX IF NOT EXISTS` on the
/// queue table, which takes a SHARE lock (blocking every writer) before it
/// notices the index exists. The existence check keeps a start-up
/// `prepare()` of a live queue away from that, and the advisory lock makes two
/// processes creating the same queue take turns.
pub(super) const CREATE_QUEUE: &str = "SELECT pgmq.create($1::text)
       FROM (SELECT pg_advisory_xact_lock(hashtextextended('pl_pgmq_create:' || $1::text, 0))) l
      WHERE to_regclass(format('pgmq.%I', 'q_' || $1::text)) IS NULL";

/// Apply the per-table autovacuum settings (cr-36) to one pgmq queue's table.
/// Idempotent; defined in the migration. `$1` queue.
pub(super) const TUNE_QUEUE: &str = "SELECT pl_pgmq_tune_queue($1::text)";

/// Statements that depend on the lane's table, plus the depth query.
pub(super) struct Statements {
    pub lanes: [Lane; LANES],
    /// `$1` logical queue, `$2` dead-letter pgmq queue. Returns
    /// `(ready, delayed, leased, dead)`.
    pub depth: SqlStr,
    /// `$1` limit. Returns the number of live entries, counted up to the limit
    /// per lane.
    pub live: SqlStr,
}

/// Statements keyed by a lease token. In all of them `$1` is the lane's pgmq
/// queue, `$2` the message id, `$3` the fence (`read_ct`), `$4` the logical
/// queue and `$5` the entry id.
pub(super) struct Lane {
    pub ack: SqlStr,
    /// `$6` delay in seconds.
    pub nack_requeue: SqlStr,
    /// `$6` reason, `$7` dead-letter pgmq queue.
    pub dead_letter: SqlStr,
    /// `$6` lease in seconds.
    pub extend: SqlStr,
}

fn dynamic(sql: String) -> SqlStr {
    AssertSqlSafe(sql).into_sql_str()
}

/// The fence guard: the message row, locked, only while this lease holds it.
///
/// Three conditions make a token usable: `read_ct` is the token's fence (no
/// later delivery), `vt` is still in the future (the lease has not run out) and
/// the entry is not delayed by a nack. The third one matters because a delayed
/// nack sets `vt` to the end of the delay without touching `read_ct`: from the
/// guard's point of view the message then looks leased again, to the token
/// that was just nacked. The nack records the end of the delay in
/// `pl_pgmq_aux.delayed_until` in the same statement, and a row with a delay
/// still running is never held. The delay is read through
/// `pl_pgmq_is_delayed`, not a subquery, so that a statement which waited for
/// the row lock re-reads it with a fresh snapshot (see the module docs). Once the delay has elapsed `vt` has too, and
/// the next delivery changes `read_ct`, so the old token stays dead for good.
fn held(table: &str) -> String {
    format!(
        "held AS (SELECT msg_id, read_ct, message FROM pgmq.\"{table}\"
                  WHERE msg_id = $2 AND read_ct = $3 AND vt > clock_timestamp()
                    AND NOT pl_pgmq_is_delayed($4, $5)
                  FOR UPDATE)"
    )
}

/// Forget the entry's side-table row (idempotency key, delay mark).
const FORGET: &str = "forgot AS (DELETE FROM pl_pgmq_aux
                        WHERE queue_name = $4 AND entry_id = $5
                          AND EXISTS (SELECT 1 FROM held))";

fn lane_statements(table: &str) -> Lane {
    let held = held(table);
    let ack = format!(
        "WITH {held},
         gone AS (SELECT pgmq.delete($1::text, held.msg_id) AS ok FROM held),
         {FORGET}
         SELECT count(*) FROM gone WHERE gone.ok"
    );
    // An immediate requeue wakes idle dispatchers; a delayed one does not (it
    // is not leasable yet). The delay mark lets `depth` tell a delayed entry from a leased one: both
    // are invisible, only a nack puts `delayed_until` in the future.
    // `due` is read once: `vt` and `delayed_until` must be the same instant, or
    // a fresh delivery right at `vt` would see a delay that is still running.
    let nack_requeue = format!(
        "WITH {held},
         due AS (SELECT clock_timestamp() + make_interval(secs => $6::float8) AS at),
         moved AS (SELECT s.msg_id,
                          CASE WHEN $6::float8 <= 0
                               THEN pg_notify('pl_lease_queue', $4)::text END AS woke
                     FROM held, due,
                          LATERAL pgmq.set_vt($1::text, held.msg_id, due.at) s),
         marked AS (INSERT INTO pl_pgmq_aux (queue_name, entry_id, delayed_until)
                    SELECT $4, $5, due.at
                      FROM held, due WHERE $6::float8 > 0
                    ON CONFLICT (queue_name, entry_id)
                    DO UPDATE SET delayed_until = EXCLUDED.delayed_until),
         cleared AS (UPDATE pl_pgmq_aux SET delayed_until = NULL
                      WHERE queue_name = $4 AND entry_id = $5 AND $6::float8 <= 0
                        AND EXISTS (SELECT 1 FROM held))
         SELECT count(*) FROM moved"
    );
    // pgmq has no per-message dead-letter reason: the message moves to the
    // queue's dead-letter pgmq queue with the reason in the body.
    let dead_letter = format!(
        "WITH {held},
         dead AS (SELECT pgmq.send($7::text, jsonb_build_object(
                         'entry', $5::bigint, 'queue', $4::text, 'reason', $6::text,
                         'attempts', held.read_ct, 'task', held.message)) AS id
                    FROM held),
         gone AS (SELECT pgmq.delete($1::text, held.msg_id) AS ok FROM held),
         {FORGET}
         SELECT count(*) FROM dead, gone WHERE gone.ok"
    );
    let extend = format!(
        "WITH {held},
         moved AS (SELECT s.msg_id
                     FROM held,
                          LATERAL pgmq.set_vt($1::text, held.msg_id,
                                  clock_timestamp() + make_interval(secs => $6::float8)) s)
         SELECT count(*) FROM moved"
    );
    Lane {
        ack: dynamic(ack),
        nack_requeue: dynamic(nack_requeue),
        dead_letter: dynamic(dead_letter),
        extend: dynamic(extend),
    }
}

/// Build every dynamic statement for the pgmq lane tables `tables`
/// (`q_<pgmq queue name>`).
pub(super) fn build(tables: &[String; LANES]) -> Statements {
    let lanes = [
        lane_statements(&tables[0]),
        lane_statements(&tables[1]),
        lane_statements(&tables[2]),
        lane_statements(&tables[3]),
    ];

    let states = tables
        .iter()
        .enumerate()
        .map(|(lane, table)| {
            format!(
                "SELECT count(*) FILTER (WHERE q.vt <= clock_timestamp()) AS ready,
                        count(*) FILTER (WHERE q.vt > clock_timestamp()
                                           AND a.delayed_until > clock_timestamp()) AS delayed,
                        count(*) FILTER (WHERE q.vt > clock_timestamp()
                                           AND NOT coalesce(a.delayed_until > clock_timestamp(), false)) AS leased
                   FROM pgmq.\"{table}\" q
                   LEFT JOIN pl_pgmq_aux a
                          ON a.queue_name = $1 AND a.entry_id = q.msg_id * {LANES} + {lane}"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let depth = format!(
        "SELECT coalesce(sum(ready), 0)::bigint, coalesce(sum(delayed), 0)::bigint,
                coalesce(sum(leased), 0)::bigint,
                (SELECT queue_length FROM pgmq.metrics($2::text))
           FROM ({states}) s"
    );

    let live = tables
        .iter()
        .map(|table| {
            format!("SELECT count(*) AS n FROM (SELECT 1 FROM pgmq.\"{table}\" LIMIT $1) l")
        })
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    let live = format!("SELECT coalesce(sum(n), 0)::bigint FROM ({live}) s");

    Statements {
        lanes,
        depth: dynamic(depth),
        live: dynamic(live),
    }
}
