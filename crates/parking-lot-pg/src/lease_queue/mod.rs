//! Postgres lease queue: the durable [`LeaseQueue`] backend on raw sqlx
//! (cr-26).
//!
//! # Guarantees
//!
//! - **At-least-once.** A leased entry stays in the table until it is acked.
//!   A consumer that dies (even `kill -9`) leaves it leased; once the lease
//!   expires the next `lease` call, or the [reaper](PgLeaseQueue::reap_expired),
//!   returns it to the ready set with a higher attempt count.
//! - **No double claim.** Candidates are locked with `FOR UPDATE SKIP LOCKED`
//!   in one statement, so concurrent dispatchers (in one process or many) never
//!   hold the same lease. The statement-level argument and the isolation level
//!   of each statement are in the `sql` module's documentation.
//! - **Fencing.** The lease token carries the entry's attempt number. Ack,
//!   nack and extend match on it, so a consumer that outlived its lease gets
//!   [`SchedulerError::LeaseLost`] even before the entry is redelivered.
//! - **Time is the database's.** Leases and backoff use `now()` on the server.
//!
//! # Running it
//!
//! Apply the schema once with [`PgLeaseQueue::migrate`] (idempotent), build a
//! queue per logical queue name, and share the pool:
//!
//! ```ignore
//! PgLeaseQueue::<MyPayload>::migrate(&pool).await?;
//! let queue = PgLeaseQueue::<MyPayload>::new(pool, "inference", 10_000)
//!     .with_max_attempts(5);
//! let _reaper = queue.spawn_reaper(Duration::from_secs(5));
//!
//! let mut wake = queue.listener().await?;           // listen before leasing
//! loop {
//!     let batch = queue.lease(16, Duration::from_secs(30)).await?;
//!     if batch.is_empty() {
//!         wake.wait(Duration::from_secs(5)).await?;  // woken by NOTIFY
//!     }
//!     // run each task, heartbeat with `extend`, then `ack` or `nack`
//! }
//! ```
//!
//! Entries are ordered by priority (highest first), then by enqueue `id`.
//! `id` is assigned at insert, so two enqueues that commit out of order can be
//! leased in commit order rather than id order.
//!
//! Poison tasks: a task that crashes its consumer never reaches `nack`.
//! [`with_max_attempts`](PgLeaseQueue::with_max_attempts) dead-letters an entry
//! once it has been leased that many times and its lease expires (or it is
//! nacked back) again.

mod sql;
mod wake;

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Enqueued, LeaseQueue, LeaseToken, LeasedTask, NackAction, Priority, QueueDepth, QueueEntryId,
    ScheduledTask, SchedulerError, TaskMetadata,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::{PgPool, Row, postgres::PgRow};

pub use self::sql::NOTIFY_CHANNEL;
use self::sql::{
    DEAD_LETTER_UNDECODABLE, ENQUEUE_COUNT, ENQUEUE_EXISTING, ENQUEUE_INSERT, ENQUEUE_LOCK,
    MIGRATION, STATEMENTS,
};
pub use self::wake::{QueueListener, Reaped, ReaperHandle, Wake};

type PgQuery = sqlx::query::Query<'static, sqlx::Postgres, sqlx::postgres::PgArguments>;

/// A durable [`LeaseQueue`] over one named queue in a Postgres table.
///
/// Cheap to clone (a pool handle and a name); clones share the same queue.
/// `P` is the task payload, stored as JSON.
pub struct PgLeaseQueue<P> {
    pool: PgPool,
    queue_name: Arc<str>,
    /// [`LeaseQueue::identity`]: `pg:<table>:<queue name>`.
    identity: Arc<str>,
    max_depth: usize,
    max_attempts: Option<u32>,
    payload: PhantomData<fn() -> P>,
}

impl<P> Clone for PgLeaseQueue<P> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            queue_name: Arc::clone(&self.queue_name),
            identity: Arc::clone(&self.identity),
            max_depth: self.max_depth,
            max_attempts: self.max_attempts,
            payload: PhantomData,
        }
    }
}

impl<P> std::fmt::Debug for PgLeaseQueue<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgLeaseQueue")
            .field("queue_name", &self.queue_name)
            .field("max_depth", &self.max_depth)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl<P> PgLeaseQueue<P> {
    /// A queue named `queue_name` that accepts at most `max_depth` live
    /// (ready, delayed or leased) entries. Queues with different names share
    /// the table but not entries.
    ///
    /// The schema must exist: call [`migrate`](Self::migrate) first.
    #[must_use]
    pub fn new(pool: PgPool, queue_name: impl Into<String>, max_depth: usize) -> Self {
        let queue_name: Arc<str> = Arc::from(queue_name.into());
        Self {
            pool,
            identity: Arc::from(format!("pg:{}:{queue_name}", self::sql::TABLE)),
            queue_name,
            max_depth,
            max_attempts: None,
            payload: PhantomData,
        }
    }

    /// Dead-letter an entry that has been leased `max_attempts` times instead
    /// of leasing it again. Unset by default (redeliver forever).
    #[must_use]
    pub const fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }

    /// The queue's name.
    #[must_use]
    pub fn queue_name(&self) -> &str {
        &self.queue_name
    }

    /// Create the table, indexes and wake-up triggers if they are missing.
    /// Idempotent, and safe to call from several processes at once.
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

    /// Subscribe to this queue's wake-up notifications. See [`QueueListener`].
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the listener connection cannot be
    /// opened.
    pub async fn listener(&self) -> Result<QueueListener, SchedulerError> {
        QueueListener::connect(&self.pool, self.queue_name.to_string()).await
    }

    /// Return expired leases to the ready set (or dead-letter them when the
    /// entry used up its attempts), notifying listeners.
    ///
    /// The queue is correct without the reaper: `lease` reclaims expired
    /// leases itself and every operation treats an expired lease as lost. The
    /// reaper adds the wake-up (an expiry is not an insert, so idle dispatchers
    /// would otherwise only notice on their poll timer) and keeps state-based
    /// monitoring of the table honest.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] on a storage failure.
    pub async fn reap_expired(&self) -> Result<Reaped, SchedulerError> {
        let (requeued, dead): (i64, i64) = sqlx::query_as(STATEMENTS.reap)
            .bind(&*self.queue_name)
            .bind(self.max_attempts_bind())
            .fetch_one(&self.pool)
            .await
            .map_err(backend("reap"))?;
        Ok(Reaped {
            requeued: count(requeued),
            dead_lettered: count(dead),
        })
    }

    fn max_attempts_bind(&self) -> Option<i32> {
        self.max_attempts
            .map(|n| i32::try_from(n).unwrap_or(i32::MAX))
    }

    /// Run a statement keyed by a lease token; zero rows means the token no
    /// longer names a held lease.
    async fn fenced(
        &self,
        statement: &'static str,
        token: LeaseToken,
        extra: impl FnOnce(PgQuery) -> PgQuery + Send,
        context: &'static str,
    ) -> Result<(), SchedulerError> {
        let Some((id, fence)) = token_parts(token) else {
            return Err(token.lost());
        };
        let query = sqlx::query(statement)
            .bind(self.queue_name.to_string())
            .bind(id)
            .bind(fence);
        let done = extra(query)
            .execute(&self.pool)
            .await
            .map_err(backend(context))?;
        if done.rows_affected() == 0 {
            return Err(token.lost());
        }
        Ok(())
    }
}

impl<P> LeaseQueue<P> for PgLeaseQueue<P>
where
    P: Serialize + DeserializeOwned + Send + Sync,
{
    /// Stores the entry in a short transaction guarded by a per-queue advisory
    /// lock, so `max_depth` is exact and an idempotency key is stored once.
    /// Dropping the future rolls the transaction back.
    async fn enqueue(&self, task: ScheduledTask<P>) -> Result<Enqueued, SchedulerError> {
        let meta =
            serde_json::to_value(&task.meta).map_err(not_stored("serialize task metadata"))?;
        let payload =
            serde_json::to_value(&task.payload).map_err(not_stored("serialize task payload"))?;
        let priority = priority_to_i16(task.meta.priority);
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
            if let Some(id) = existing {
                return Ok(Enqueued::Duplicate(entry_id(id)?));
            }
        }

        let limit = i64::try_from(self.max_depth).unwrap_or(i64::MAX);
        let live: i64 = sqlx::query_scalar(ENQUEUE_COUNT)
            .bind(queue)
            .bind(limit)
            .fetch_one(&mut *tx)
            .await
            .map_err(not_stored("enqueue depth check"))?;
        if usize::try_from(live).unwrap_or(usize::MAX) >= self.max_depth {
            return Err(SchedulerError::queue_full(
                queue,
                usize::try_from(live).unwrap_or(usize::MAX),
                self.max_depth,
            ));
        }

        let id: i64 = sqlx::query_scalar(ENQUEUE_INSERT)
            .bind(queue)
            .bind(priority)
            .bind(key)
            .bind(meta)
            .bind(payload)
            .fetch_one(&mut *tx)
            .await
            .map_err(not_stored("enqueue insert"))?;
        tx.commit().await.map_err(backend("enqueue commit"))?;
        Ok(Enqueued::Accepted(entry_id(id)?))
    }

    /// One statement (see the `sql` module for why it cannot double-claim). An entry whose stored task cannot be
    /// decoded (for example after a payload type change) is dead-lettered with
    /// the decode error as its reason instead of wedging every later lease.
    async fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> Result<Vec<LeasedTask<P>>, SchedulerError> {
        ensure_duration(lease_for, "lease duration")?;
        if max == 0 {
            return Ok(Vec::new());
        }
        let rows = sqlx::query(STATEMENTS.lease)
            .bind(&*self.queue_name)
            .bind(i64::try_from(max).unwrap_or(i64::MAX))
            .bind(lease_for.as_secs_f64())
            .bind(self.max_attempts_bind())
            .fetch_all(&self.pool)
            .await
            .map_err(backend("lease"))?;

        let mut leased = Vec::with_capacity(rows.len());
        for row in &rows {
            let (id, priority, attempts) = row_keys(row)?;
            match decode_task::<P>(row) {
                Ok(task) => {
                    let token = LeaseToken::new(entry_id(id)?, u64::from(attempts));
                    leased.push((priority, id, LeasedTask::new(task, token, attempts)));
                }
                Err(reason) => {
                    // Not retryable: redelivery would fail the same way.
                    // A failed dead-letter write is logged, not returned: the
                    // tasks this call already leased must reach the caller.
                    // The entry stays leased and is reclaimed when it expires.
                    if let Err(error) = sqlx::query(DEAD_LETTER_UNDECODABLE)
                        .bind(&*self.queue_name)
                        .bind(id)
                        .bind(i32::try_from(attempts).unwrap_or(i32::MAX))
                        .bind(format!("undecodable task: {reason}"))
                        .execute(&self.pool)
                        .await
                    {
                        tracing::warn!(
                            %error, queue = &*self.queue_name, id,
                            "could not dead-letter an undecodable entry; it stays leased until its lease expires"
                        );
                    }
                }
            }
        }
        // RETURNING does not preserve the candidate order.
        leased.sort_by_key(|(priority, id, _)| (std::cmp::Reverse(*priority), *id));
        Ok(leased.into_iter().map(|(_, _, task)| task).collect())
    }

    async fn ack(&self, token: LeaseToken) -> Result<(), SchedulerError> {
        self.fenced(STATEMENTS.ack, token, |q| q, "ack").await
    }

    async fn nack(&self, token: LeaseToken, action: NackAction) -> Result<(), SchedulerError> {
        match action {
            NackAction::Requeue { delay } => {
                let secs = delay.as_secs_f64();
                self.fenced(
                    STATEMENTS.nack_requeue,
                    token,
                    move |q| q.bind(secs),
                    "nack",
                )
                .await
            }
            NackAction::DeadLetter { reason } => {
                self.fenced(
                    STATEMENTS.nack_dead_letter,
                    token,
                    move |q| q.bind(reason),
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
        self.fenced(STATEMENTS.extend, token, move |q| q.bind(secs), "extend")
            .await
    }

    async fn depth(&self) -> Result<QueueDepth, SchedulerError> {
        let (ready, delayed, leased, dead): (i64, i64, i64, i64) = sqlx::query_as(STATEMENTS.depth)
            .bind(&*self.queue_name)
            .fetch_one(&self.pool)
            .await
            .map_err(backend("depth"))?;
        Ok(QueueDepth::new(
            usize::try_from(ready).unwrap_or(usize::MAX),
            usize::try_from(delayed).unwrap_or(usize::MAX),
            usize::try_from(leased).unwrap_or(usize::MAX),
            usize::try_from(dead).unwrap_or(usize::MAX),
        ))
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// `pg:<table>:<queue name>`: entry ids come from the table's sequence.
    /// Two databases with a queue of the same name have the same identity, so
    /// do not deliver both into one mailbox without task idempotency keys.
    fn identity(&self) -> &str {
        &self.identity
    }
}

/// Maps a driver error to [`SchedulerError::Backend`] naming the operation.
fn backend<E>(operation: &'static str) -> impl FnOnce(E) -> SchedulerError
where
    E: std::error::Error + Send + Sync + 'static,
{
    move |source| SchedulerError::backend(format!("pg lease queue: {operation}"), source)
}

/// A failure before the enqueue's commit: the transaction is rolled back, so
/// nothing was stored and the pool can report the task as refused.
fn not_stored<E>(operation: &'static str) -> impl FnOnce(E) -> SchedulerError
where
    E: std::error::Error + Send + Sync + 'static,
{
    move |source| SchedulerError::not_stored(format!("pg lease queue: {operation}"), source)
}

fn ensure_duration(duration: Duration, what: &str) -> Result<(), SchedulerError> {
    if duration.is_zero() {
        return Err(SchedulerError::InvalidConfig {
            message: format!("{what} must be greater than zero"),
        });
    }
    Ok(())
}

const fn priority_to_i16(priority: Priority) -> i16 {
    match priority {
        Priority::Low => 0,
        Priority::Normal => 1,
        Priority::High => 2,
        Priority::Critical => 3,
    }
}

fn entry_id(id: i64) -> Result<QueueEntryId, SchedulerError> {
    u64::try_from(id)
        .map(QueueEntryId)
        .map_err(|_| SchedulerError::internal(format!("negative queue entry id {id}")))
}

/// The `(id, fence)` a token names, or `None` when they cannot match any row
/// (the token was not minted by this backend).
fn token_parts(token: LeaseToken) -> Option<(i64, i32)> {
    let id = i64::try_from(token.entry().0).ok()?;
    let fence = i32::try_from(token.fence()).ok()?;
    Some((id, fence))
}

fn count(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// `(id, priority, attempts)` of a leased row.
fn row_keys(row: &PgRow) -> Result<(i64, i16, u32), SchedulerError> {
    let id: i64 = row.try_get("id").map_err(backend("decode row id"))?;
    let priority: i16 = row
        .try_get("priority")
        .map_err(backend("decode row priority"))?;
    let attempts: i32 = row
        .try_get("attempts")
        .map_err(backend("decode row attempts"))?;
    let attempts = u32::try_from(attempts)
        .map_err(|_| SchedulerError::internal(format!("negative attempt count {attempts}")))?;
    Ok((id, priority, attempts))
}

/// Decode the stored metadata and payload; the error is a human-readable
/// reason, not a storage failure.
fn decode_task<P: DeserializeOwned>(row: &PgRow) -> Result<ScheduledTask<P>, String> {
    let meta: serde_json::Value = row.try_get("meta").map_err(|e| e.to_string())?;
    let payload: serde_json::Value = row.try_get("payload").map_err(|e| e.to_string())?;
    let meta: TaskMetadata = serde_json::from_value(meta).map_err(|e| format!("metadata: {e}"))?;
    let payload: P = serde_json::from_value(payload).map_err(|e| format!("payload: {e}"))?;
    Ok(ScheduledTask { meta, payload })
}
