//! pgmq lease queue: a [`LeaseQueue`] over the pgmq extension, reached through
//! plain SQL on raw sqlx (cr-30). No pgmq client crate.
//!
//! # How the contract maps onto pgmq (pgmq 1.13.1)
//!
//! | Contract | pgmq |
//! |---|---|
//! | priority | one pgmq queue per level (`pl_<name>_p0` .. `_p3`); `lease` reads the highest non-empty queue first |
//! | `enqueue` | `pgmq.send` in a transaction with a per-queue advisory lock |
//! | `lease` | `pgmq.read(queue, vt, qty)`: the visibility timeout **is** the lease |
//! | fence | `read_ct`, which `pgmq.read` increments on every delivery |
//! | `extend` | `pgmq.set_vt(queue, id, now + lease)` |
//! | `ack` | `pgmq.delete` |
//! | `nack(Requeue)` | `pgmq.set_vt(queue, id, now + delay)`; message id, so queue place, and `read_ct` are kept |
//! | `nack(DeadLetter)` | `pgmq.send` to the queue's dead-letter pgmq queue (reason in the body) and `pgmq.delete`, one statement |
//! | max attempts | `read_ct` after a `read` greater than the limit: the message is dead-lettered instead of returned |
//! | idempotency key | side table `pl_pgmq_aux`, first write wins under the advisory lock |
//!
//! The entry id is `msg_id * 4 + lane`: pgmq message ids are per queue, and
//! this makes them unique across the four priority queues. A lease token is
//! `(entry id, read_ct)`. `ack`, `nack` and `extend` lock the message row only
//! if `read_ct` still equals the fence **and** `vt` is still in the future (see
//! the `sql` module for the atomicity argument), so a token from an earlier
//! delivery, or from a lease that has run out, gets
//! [`SchedulerError::LeaseLost`].
//!
//! `ack` deletes rather than archives. `pgmq.archive` keeps every finished
//! payload forever, which is unbounded growth for a queue whose job is to be
//! drained, and would leave no way to count dead letters separately. Dead
//! letters live in their own pgmq queue, which an operator can read, replay or
//! purge with ordinary pgmq calls; `depth().dead_lettered` is that queue's
//! length.
//!
//! # What pgmq cannot do that [`PgLeaseQueue`](crate::PgLeaseQueue) can
//!
//! - **Priority is by lane, not a total order.** `lease` drains the lanes
//!   highest first, one statement per lane. The order within a lane is exact
//!   (`msg_id`), and a higher lane is always tried first, but the lanes are not
//!   read in one snapshot: a critical task that commits while a `lease` call is
//!   already reading the lower lanes is picked up by the next call. The SQL
//!   queue picks across priorities in one statement.
//! - **Lease granularity is one second.** `pgmq.read` takes an integer number
//!   of seconds, so a sub-second lease is rounded up. `nack` delays and
//!   `extend` use exact timestamps.
//! - **No wake-up on expiry or delay.** Enqueue sends a `NOTIFY` (delivered at
//!   commit, on the shared `pl_lease_queue` channel with the queue name as
//!   payload; see [`QueueListener`]). A lease that expires or a delay that
//!   elapses is just a clock passing, so idle dispatchers must also poll;
//!   [`Wake::TimedOut`](crate::Wake::TimedOut) is the cue.
//! - **No reaper needed, none provided.** An expired lease is visible to the
//!   next `read` by itself. Max attempts is enforced when a message is next
//!   leased rather than when its lease expires, so a poison message sits
//!   visible (and counts as `ready`) until a dispatcher picks it up.
//! - **No cross-statement transaction with your own tables.** The queue runs on
//!   the pool; you cannot enqueue inside your own transaction.
//! - **Idempotency needs the side table.** pgmq has no keys. A key lives in
//!   `pl_pgmq_aux` while the entry is live. Deleting a message with raw pgmq
//!   calls (not through this queue) leaves its key behind.
//! - **Crash durability of a *pool* on top is not claimed** (adopted contract,
//!   S-18): this module proves lease redelivery, fencing and priority order,
//!   the same as the SQL queue's, but the durable dispatch loop (cr-49) is
//!   validated on [`PgLeaseQueue`](crate::PgLeaseQueue).
//!
//! # Running it
//!
//! ```ignore
//! let queue = PgmqLeaseQueue::<MyPayload>::new(pool, "inference", 10_000)?
//!     .with_max_attempts(5);
//! queue.prepare().await?;                    // extension, side table, pgmq queues
//! let mut wake = queue.listener().await?;    // listen before leasing
//! loop {
//!     let batch = queue.lease(16, Duration::from_secs(30)).await?;
//!     if batch.is_empty() {
//!         wake.wait(Duration::from_secs(5)).await?;
//!     }
//!     // run each task, heartbeat with `extend`, then `ack` or `nack`
//! }
//! ```

mod sql;

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Enqueued, LeaseQueue, LeaseToken, LeasedTask, NackAction, Priority, QueueDepth, QueueEntryId,
    ScheduledTask, SchedulerError, TaskMetadata,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::{PgPool, Row, SqlStr};

use self::sql::{
    CREATE_QUEUE, ENQUEUE_EXISTING, ENQUEUE_KEY, ENQUEUE_LOCK, ENQUEUE_NOTIFY, ENQUEUE_SEND, LANES,
    LEASE_READ, MIGRATION, Statements, TUNE_QUEUE,
};
use crate::lease_queue::QueueListener;

/// Longest logical queue name: `pl_` + name + `_dead` must fit pgmq's 47
/// characters.
const MAX_NAME_LEN: usize = 38;

/// A durable [`LeaseQueue`] over pgmq. See the [module docs](self) for the
/// mapping and its limits.
///
/// Cheap to clone; clones share the same queue. `P` is the task payload,
/// stored as JSON.
pub struct PgmqLeaseQueue<P> {
    pool: PgPool,
    queue_name: Arc<str>,
    /// [`LeaseQueue::identity`]: `pgmq:<queue name>`.
    identity: Arc<str>,
    /// pgmq queue names, indexed by lane.
    lanes: Arc<[String; LANES]>,
    dead_queue: Arc<str>,
    statements: Arc<Statements>,
    max_depth: usize,
    max_attempts: Option<u32>,
    payload: PhantomData<fn() -> P>,
}

impl<P> Clone for PgmqLeaseQueue<P> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            queue_name: Arc::clone(&self.queue_name),
            identity: Arc::clone(&self.identity),
            lanes: Arc::clone(&self.lanes),
            dead_queue: Arc::clone(&self.dead_queue),
            statements: Arc::clone(&self.statements),
            max_depth: self.max_depth,
            max_attempts: self.max_attempts,
            payload: PhantomData,
        }
    }
}

impl<P> std::fmt::Debug for PgmqLeaseQueue<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgmqLeaseQueue")
            .field("queue_name", &self.queue_name)
            .field("max_depth", &self.max_depth)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl<P> PgmqLeaseQueue<P> {
    /// A queue named `queue_name` that accepts at most `max_depth` live
    /// (ready, delayed or leased) entries.
    ///
    /// The name becomes part of SQL identifiers, so it must be 1 to 38
    /// characters of `a-z`, `0-9` and `_`. Call [`prepare`](Self::prepare)
    /// before first use.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::InvalidConfig`] for a name that is empty, too long or
    /// contains other characters.
    pub fn new(
        pool: PgPool,
        queue_name: impl Into<String>,
        max_depth: usize,
    ) -> Result<Self, SchedulerError> {
        let name = queue_name.into();
        let valid = !name.is_empty()
            && name.len() <= MAX_NAME_LEN
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !valid {
            return Err(SchedulerError::InvalidConfig {
                message: format!(
                    "pgmq queue name {name:?} must be 1 to {MAX_NAME_LEN} characters of a-z, 0-9 and _"
                ),
            });
        }
        let lanes: [String; LANES] = std::array::from_fn(|lane| format!("pl_{name}_p{lane}"));
        let tables: [String; LANES] = std::array::from_fn(|lane| format!("q_{}", lanes[lane]));
        Ok(Self {
            pool,
            dead_queue: Arc::from(format!("pl_{name}_dead")),
            statements: Arc::new(sql::build(&tables)),
            lanes: Arc::new(lanes),
            identity: Arc::from(format!("pgmq:{name}")),
            queue_name: Arc::from(name),
            max_depth,
            max_attempts: None,
            payload: PhantomData,
        })
    }

    /// Dead-letter a message once it has been leased `max_attempts` times and
    /// is leased again. Unset by default (redeliver forever).
    #[must_use]
    pub const fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }

    /// The logical queue's name.
    #[must_use]
    pub fn queue_name(&self) -> &str {
        &self.queue_name
    }

    /// Install the pgmq extension and side table if missing and create this
    /// queue's pgmq queues. Idempotent and safe to run from several processes.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when a statement fails, for example when
    /// pgmq is not installed in the database.
    pub async fn prepare(&self) -> Result<(), SchedulerError> {
        Self::migrate(&self.pool).await?;
        for name in self
            .lanes
            .iter()
            .map(String::as_str)
            .chain([&*self.dead_queue])
        {
            sqlx::query(CREATE_QUEUE)
                .bind(name)
                .execute(&self.pool)
                .await
                .map_err(backend("create pgmq queue"))?;
            sqlx::query(TUNE_QUEUE)
                .bind(name)
                .execute(&self.pool)
                .await
                .map_err(backend("tune pgmq queue autovacuum"))?;
        }
        Ok(())
    }

    /// Apply the schema shared by every `PgmqLeaseQueue` (extension and side
    /// table). [`prepare`](Self::prepare) calls this.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the migration fails.
    pub async fn migrate(pool: &PgPool) -> Result<(), SchedulerError> {
        sqlx::raw_sql(MIGRATION)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(backend("migrate"))
    }

    /// Subscribe to enqueue notifications for this queue. See
    /// [`QueueListener`]; only enqueue notifies (see the module docs).
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the listener connection cannot be
    /// opened.
    pub async fn listener(&self) -> Result<QueueListener, SchedulerError> {
        QueueListener::connect(&self.pool, self.queue_name.to_string()).await
    }

    fn max_attempts_i64(&self) -> Option<i64> {
        self.max_attempts.map(i64::from)
    }

    /// Run a statement keyed by a lease token; zero rows means the token no
    /// longer names a held lease.
    async fn fenced(
        &self,
        statement: fn(&sql::Lane) -> &SqlStr,
        token: LeaseToken,
        bind: impl FnOnce(PgQuery) -> PgQuery + Send,
        context: &'static str,
    ) -> Result<(), SchedulerError> {
        let Some(parts) = EntryParts::of_token(token) else {
            return Err(token.lost());
        };
        let statement = statement(&self.statements.lanes[parts.lane]).clone();
        let query = sqlx::query_scalar::<_, i64>(statement)
            .bind(&*self.lanes[parts.lane])
            .bind(parts.msg_id)
            .bind(parts.fence)
            .bind(&*self.queue_name)
            .bind(parts.entry);
        let changed: i64 = bind(query)
            .fetch_one(&self.pool)
            .await
            .map_err(backend(context))?;
        if changed == 0 {
            return Err(token.lost());
        }
        Ok(())
    }

    /// Dead-letter a message this call just leased. A failure is logged, not
    /// returned: the tasks this call already leased must reach the caller, or
    /// they would sit invisible until their leases ran out. The message that
    /// could not be dead-lettered stays leased and is delivered again (and
    /// dead-lettered again) when its lease expires.
    async fn dead_letter_or_log(&self, lane: usize, msg_id: i64, read_ct: i32, reason: String) {
        if let Err(error) = self.dead_letter_leased(lane, msg_id, read_ct, reason).await {
            tracing::warn!(
                %error, queue = &*self.queue_name, msg_id,
                "could not dead-letter a leased message; it stays leased until its lease expires"
            );
        }
    }

    async fn dead_letter_leased(
        &self,
        lane: usize,
        msg_id: i64,
        read_ct: i32,
        reason: String,
    ) -> Result<(), SchedulerError> {
        sqlx::query_scalar::<_, i64>(self.statements.lanes[lane].dead_letter.clone())
            .bind(&*self.lanes[lane])
            .bind(msg_id)
            .bind(read_ct)
            .bind(&*self.queue_name)
            .bind(entry_of(msg_id, lane))
            .bind(reason)
            .bind(&*self.dead_queue)
            .fetch_one(&self.pool)
            .await
            .map(|_| ())
            .map_err(backend("dead-letter"))
    }
}

type PgQuery<'q> = sqlx::query::QueryScalar<'q, sqlx::Postgres, i64, sqlx::postgres::PgArguments>;

impl<P> LeaseQueue<P> for PgmqLeaseQueue<P>
where
    P: Serialize + DeserializeOwned + Send + Sync,
{
    /// Stores the message in a short transaction guarded by a per-queue
    /// advisory lock, so `max_depth` is exact and an idempotency key is stored
    /// once. Dropping the future rolls the transaction back.
    async fn enqueue(&self, task: ScheduledTask<P>) -> Result<Enqueued, SchedulerError> {
        let message = serde_json::json!({
            "meta": serde_json::to_value(&task.meta).map_err(not_stored("serialize task metadata"))?,
            "payload": serde_json::to_value(&task.payload).map_err(not_stored("serialize task payload"))?,
        });
        let lane = sql::lane_of(priority_level(task.meta.priority));
        let key = task.meta.idempotency_key;
        let queue = &*self.queue_name;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(not_stored("enqueue begin"))?;
        sqlx::query(ENQUEUE_LOCK)
            .bind(queue)
            .execute(&mut *tx)
            .await
            .map_err(not_stored("enqueue lock"))?;

        if let Some(key) = &key {
            let existing: Option<i64> = sqlx::query_scalar(ENQUEUE_EXISTING)
                .bind(queue)
                .bind(key)
                .fetch_optional(&mut *tx)
                .await
                .map_err(not_stored("enqueue idempotency check"))?;
            if let Some(entry) = existing {
                return Ok(Enqueued::Duplicate(entry_id(entry)?));
            }
        }

        let limit = i64::try_from(self.max_depth).unwrap_or(i64::MAX);
        let live: i64 = sqlx::query_scalar(self.statements.live.clone())
            .bind(limit)
            .fetch_one(&mut *tx)
            .await
            .map_err(not_stored("enqueue depth check"))?;
        let live = usize::try_from(live).unwrap_or(usize::MAX);
        if live >= self.max_depth {
            return Err(SchedulerError::queue_full(queue, live, self.max_depth));
        }

        let msg_id: i64 = sqlx::query_scalar(ENQUEUE_SEND)
            .bind(&*self.lanes[lane])
            .bind(message)
            .fetch_one(&mut *tx)
            .await
            .map_err(not_stored("enqueue send"))?;
        let entry = entry_of(msg_id, lane);
        if let Some(key) = &key {
            sqlx::query(ENQUEUE_KEY)
                .bind(queue)
                .bind(entry)
                .bind(key)
                .execute(&mut *tx)
                .await
                .map_err(not_stored("enqueue key"))?;
        }
        sqlx::query(ENQUEUE_NOTIFY)
            .bind(queue)
            .execute(&mut *tx)
            .await
            .map_err(not_stored("enqueue notify"))?;
        tx.commit().await.map_err(backend("enqueue commit"))?;
        Ok(Enqueued::Accepted(entry_id(entry)?))
    }

    /// Reads the lanes from the highest priority down, each with one
    /// `pgmq.read` (which claims with `FOR UPDATE SKIP LOCKED`, so concurrent
    /// dispatchers never receive the same message). A message past
    /// `max_attempts`, or one that cannot be decoded, is dead-lettered and
    /// does not count toward `max`.
    ///
    /// If reading a lower lane fails after higher lanes already leased tasks,
    /// those tasks are returned and the error is logged; the error is returned
    /// only when nothing was leased.
    async fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> Result<Vec<LeasedTask<P>>, SchedulerError> {
        ensure_duration(lease_for, "lease duration")?;
        let vt = lease_seconds(lease_for);
        let mut leased = Vec::new();
        match self.lease_lanes(max, vt, &mut leased).await {
            Ok(()) => Ok(leased),
            // Messages already read are invisible to everyone else until their
            // leases run out: returning the error would strand them for a whole
            // lease. Hand them over; the failing lane is read again next call.
            Err(error) if !leased.is_empty() => {
                tracing::error!(
                    %error, queue = &*self.queue_name, leased = leased.len(),
                    "lease failed on a lower lane; returning the tasks already leased"
                );
                Ok(leased)
            }
            Err(error) => Err(error),
        }
    }

    async fn ack(&self, token: LeaseToken) -> Result<(), SchedulerError> {
        self.fenced(|lane| &lane.ack, token, |q| q, "ack").await
    }

    async fn nack(&self, token: LeaseToken, action: NackAction) -> Result<(), SchedulerError> {
        match action {
            NackAction::Requeue { delay } => {
                let secs = delay.as_secs_f64();
                self.fenced(
                    |lane| &lane.nack_requeue,
                    token,
                    move |q| q.bind(secs),
                    "nack",
                )
                .await
            }
            NackAction::DeadLetter { reason } => {
                let dead = self.dead_queue.to_string();
                self.fenced(
                    |lane| &lane.dead_letter,
                    token,
                    move |q| q.bind(reason).bind(dead),
                    "nack",
                )
                .await
            }
            other => Err(SchedulerError::InvalidConfig {
                message: format!("unsupported nack action {other:?}"),
            }),
        }
    }

    async fn extend(&self, token: LeaseToken, lease_for: Duration) -> Result<(), SchedulerError> {
        ensure_duration(lease_for, "lease duration")?;
        let secs = lease_for.as_secs_f64();
        self.fenced(|lane| &lane.extend, token, move |q| q.bind(secs), "extend")
            .await
    }

    async fn depth(&self) -> Result<QueueDepth, SchedulerError> {
        let (ready, delayed, leased, dead): (i64, i64, i64, i64) =
            sqlx::query_as(self.statements.depth.clone())
                .bind(&*self.queue_name)
                .bind(&*self.dead_queue)
                .fetch_one(&self.pool)
                .await
                .map_err(backend("depth"))?;
        let n = |v: i64| usize::try_from(v).unwrap_or(usize::MAX);
        Ok(QueueDepth::new(n(ready), n(delayed), n(leased), n(dead)))
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// `pgmq:<queue name>`: message ids are per pgmq queue. Two databases
    /// with a queue of the same name have the same identity, so do not
    /// deliver both into one mailbox without task idempotency keys.
    fn identity(&self) -> &str {
        &self.identity
    }
}

impl<P> PgmqLeaseQueue<P>
where
    P: DeserializeOwned,
{
    /// Read the lanes from the highest down into `leased` until it holds `max`
    /// tasks. `leased` keeps what was read even when a later read fails.
    async fn lease_lanes(
        &self,
        max: usize,
        vt: i32,
        leased: &mut Vec<LeasedTask<P>>,
    ) -> Result<(), SchedulerError> {
        for lane in (0..LANES).rev() {
            while leased.len() < max {
                let want = i32::try_from(max - leased.len()).unwrap_or(i32::MAX);
                let rows = sqlx::query(LEASE_READ)
                    .bind(&*self.lanes[lane])
                    .bind(vt)
                    .bind(want)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(backend("lease"))?;
                if rows.is_empty() {
                    break;
                }
                for row in &rows {
                    let msg_id: i64 = row.try_get("msg_id").map_err(backend("decode msg_id"))?;
                    let read_ct: i32 = row.try_get("read_ct").map_err(backend("decode read_ct"))?;
                    let attempts = u32::try_from(read_ct).map_err(|_| {
                        SchedulerError::internal(format!("negative read_ct {read_ct}"))
                    })?;
                    if let Some(limit) = self.max_attempts_i64()
                        && i64::from(read_ct) > limit
                    {
                        let reason = format!("max attempts ({limit}) exceeded");
                        self.dead_letter_or_log(lane, msg_id, read_ct, reason).await;
                        continue;
                    }
                    match decode_task::<P>(row) {
                        Ok(task) => {
                            let token = LeaseToken::new(
                                entry_id(entry_of(msg_id, lane))?,
                                u64::from(attempts),
                            );
                            leased.push(LeasedTask::new(task, token, attempts));
                        }
                        Err(reason) => {
                            // Not retryable: redelivery would fail the same way.
                            let reason = format!("undecodable task: {reason}");
                            self.dead_letter_or_log(lane, msg_id, read_ct, reason).await;
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

/// What a token names: lane, pgmq message id, entry id and fence, in the
/// types the statements bind.
struct EntryParts {
    lane: usize,
    msg_id: i64,
    entry: i64,
    fence: i32,
}

impl EntryParts {
    /// `None` when the token cannot name any message (it was not minted here).
    fn of_token(token: LeaseToken) -> Option<Self> {
        let entry = i64::try_from(token.entry().0).ok()?;
        let lane = usize::try_from(entry).ok()? % LANES;
        Some(Self {
            lane,
            msg_id: entry / i64::try_from(LANES).ok()?,
            entry,
            fence: i32::try_from(token.fence()).ok()?,
        })
    }
}

/// The entry id of pgmq message `msg_id` in `lane`.
fn entry_of(msg_id: i64, lane: usize) -> i64 {
    // `lane < LANES`, so the conversion cannot fail.
    msg_id * i64::try_from(LANES).unwrap_or(4) + i64::try_from(lane).unwrap_or(0)
}

fn entry_id(entry: i64) -> Result<QueueEntryId, SchedulerError> {
    u64::try_from(entry)
        .map(QueueEntryId)
        .map_err(|_| SchedulerError::internal(format!("negative queue entry id {entry}")))
}

/// 0 = low .. 3 = critical.
const fn priority_level(priority: Priority) -> usize {
    match priority {
        Priority::Low => 0,
        Priority::Normal => 1,
        Priority::High => 2,
        Priority::Critical => 3,
    }
}

/// `pgmq.read` takes whole seconds; round up so a lease is never shorter than
/// asked.
fn lease_seconds(lease_for: Duration) -> i32 {
    let secs = lease_for.as_secs() + u64::from(lease_for.subsec_nanos() > 0);
    i32::try_from(secs).unwrap_or(i32::MAX)
}

fn ensure_duration(duration: Duration, what: &str) -> Result<(), SchedulerError> {
    if duration.is_zero() {
        return Err(SchedulerError::InvalidConfig {
            message: format!("{what} must be greater than zero"),
        });
    }
    Ok(())
}

/// Maps a driver error to [`SchedulerError::Backend`] naming the operation.
fn backend<E>(operation: &'static str) -> impl FnOnce(E) -> SchedulerError
where
    E: std::error::Error + Send + Sync + 'static,
{
    move |source| SchedulerError::backend(format!("pgmq lease queue: {operation}"), source)
}

/// A failure before the enqueue's commit: the transaction is rolled back, so
/// nothing was stored and the pool can report the task as refused.
fn not_stored<E>(operation: &'static str) -> impl FnOnce(E) -> SchedulerError
where
    E: std::error::Error + Send + Sync + 'static,
{
    move |source| SchedulerError::not_stored(format!("pgmq lease queue: {operation}"), source)
}

/// Decode the stored message; the error is a human-readable reason.
fn decode_task<P: DeserializeOwned>(
    row: &sqlx::postgres::PgRow,
) -> Result<ScheduledTask<P>, String> {
    let message: serde_json::Value = row.try_get("message").map_err(|e| e.to_string())?;
    let meta = message.get("meta").cloned().ok_or("message has no meta")?;
    let payload = message
        .get("payload")
        .cloned()
        .ok_or("message has no payload")?;
    let meta: TaskMetadata = serde_json::from_value(meta).map_err(|e| format!("metadata: {e}"))?;
    let payload: P = serde_json::from_value(payload).map_err(|e| format!("payload: {e}"))?;
    Ok(ScheduledTask { meta, payload })
}
