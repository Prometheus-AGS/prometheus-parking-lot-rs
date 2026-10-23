//! Postgres result mailbox: the durable [`ResultMailbox`] backend on raw sqlx
//! (cr-27).
//!
//! # Guarantees
//!
//! - **Consume once.** `consume` is one `DELETE ... RETURNING` statement that
//!   locks its row with `FOR UPDATE SKIP LOCKED`, so two concurrent consumers
//!   never receive the same entry (the statement-level argument is in the
//!   `sql` module).
//! - **First write wins.** `deliver` claims the idempotency key and stores the
//!   entry in one `INSERT ... ON CONFLICT` statement. The key is remembered
//!   after its entry was consumed, until the entry's TTL elapses.
//! - **TTL.** An entry past its TTL is never returned, whether or not the
//!   sweep has removed it. [`evict_expired`](ResultMailbox::evict_expired)
//!   (and the background [sweeper](PgResultMailbox::spawn_sweeper)) delete
//!   expired entries and forget their keys.
//! - **Time is the database's.** TTLs use `now()` on the server.
//!
//! # Running it
//!
//! ```ignore
//! PgResultMailbox::<MyResult>::migrate(&pool).await?;       // once, idempotent
//! let mailbox = PgResultMailbox::<MyResult>::new(pool, "results");
//! let _sweeper = mailbox.spawn_sweeper(Duration::from_secs(30));
//!
//! mailbox.deliver(Delivery::new(key.clone(), task_id, TaskStatus::Completed, Some(out))
//!     .with_idempotency_key("job-17")
//!     .with_ttl(Duration::from_secs(3600))).await?;
//! let entry = mailbox.consume(&key).await?;                  // at most once
//! ```
//!
//! # Limits
//!
//! `consume` removes the entry in the statement that returns it. If the stored
//! payload no longer decodes as `T` (for example after a type change between
//! deploys) the entry is already gone when the decode fails, and the call
//! returns an error naming the entry. Keep stored result types
//! backward-compatible across the TTL window.

mod sql;

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, MailboxEntry, MailboxKey, ResultMailbox, SchedulerError, TaskStatus,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::{PgPool, Row, postgres::PgRow};
use tokio::task::JoinHandle;

use self::sql::{DELIVER_KEYED, DELIVER_PLAIN, EVICT, MIGRATION, PEEK};

#[cfg(parking_lot_negative_control = "s-7")]
pub mod negative_control;

/// A durable [`ResultMailbox`] over one named mailbox in Postgres tables.
///
/// Cheap to clone (a pool handle and a name); clones share the same mailbox.
/// Mailboxes with different names share the tables but not entries or
/// idempotency keys. `T` is the result payload, stored as JSON.
pub struct PgResultMailbox<T> {
    pool: PgPool,
    name: Arc<str>,
    payload: PhantomData<fn() -> T>,
}

impl<T> Clone for PgResultMailbox<T> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            name: Arc::clone(&self.name),
            payload: PhantomData,
        }
    }
}

impl<T> std::fmt::Debug for PgResultMailbox<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PgResultMailbox")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl<T> PgResultMailbox<T> {
    /// A mailbox named `name`.
    ///
    /// The schema must exist: call [`migrate`](Self::migrate) first.
    #[must_use]
    pub fn new(pool: PgPool, name: impl Into<String>) -> Self {
        Self {
            pool,
            name: Arc::from(name.into()),
            payload: PhantomData,
        }
    }

    /// The mailbox's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Create the tables and indexes if they are missing. Idempotent, and safe
    /// to call from several processes at once.
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
}

/// The stored form of a mailbox key: its JSON text, which is canonical because
/// the field order of [`MailboxKey`] is fixed.
fn key_text(key: &MailboxKey) -> Result<String, SchedulerError> {
    serde_json::to_string(key).map_err(backend("encode mailbox key"))
}

impl<T> ResultMailbox<T> for PgResultMailbox<T>
where
    T: Serialize + DeserializeOwned + Send + Sync,
{
    /// One statement; with a key it claims the key and stores the entry
    /// atomically. Dropping the future may or may not have stored the entry; a
    /// retry with the same key resolves to `Duplicate` if it did.
    async fn deliver(&self, delivery: Delivery<T>) -> Result<DeliveryOutcome, SchedulerError> {
        let mailbox_key = key_text(&delivery.key)?;
        let task_id = i64::try_from(delivery.task_id).map_err(|_| {
            SchedulerError::internal(format!("task id {} exceeds bigint", delivery.task_id))
        })?;
        let status = serde_json::to_value(&delivery.status).map_err(backend("encode status"))?;
        let payload = delivery
            .payload
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(backend("encode payload"))?;
        let ttl_secs = delivery.ttl.map(|ttl| ttl.as_secs_f64());

        let stored = match &delivery.idempotency_key {
            Some(idempotency_key) => {
                sqlx::query_scalar::<_, i64>(DELIVER_KEYED)
                    .bind(&*self.name)
                    .bind(&mailbox_key)
                    .bind(task_id)
                    .bind(status)
                    .bind(payload)
                    .bind(ttl_secs)
                    .bind(idempotency_key)
                    .fetch_optional(&self.pool)
                    .await
            }
            None => {
                sqlx::query_scalar::<_, i64>(DELIVER_PLAIN)
                    .bind(&*self.name)
                    .bind(&mailbox_key)
                    .bind(task_id)
                    .bind(status)
                    .bind(payload)
                    .bind(ttl_secs)
                    .fetch_optional(&self.pool)
                    .await
            }
        }
        .map_err(backend("deliver"))?;

        Ok(if stored.is_some() {
            DeliveryOutcome::Stored
        } else {
            DeliveryOutcome::Duplicate
        })
    }

    /// One statement (see the `sql` module for why it cannot return an entry
    /// twice). Dropping the future after the database removed the entry loses
    /// the entry.
    async fn consume(&self, key: &MailboxKey) -> Result<Option<MailboxEntry<T>>, SchedulerError> {
        let mailbox_key = key_text(key)?;
        let row = self.take_oldest(&mailbox_key).await?;
        row.as_ref().map(decode_entry::<T>).transpose()
    }

    async fn peek(&self, key: &MailboxKey) -> Result<Option<MailboxEntry<T>>, SchedulerError> {
        let mailbox_key = key_text(key)?;
        let row = sqlx::query(PEEK)
            .bind(&*self.name)
            .bind(&mailbox_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend("peek"))?;
        row.as_ref().map(decode_entry::<T>).transpose()
    }

    async fn evict_expired(&self) -> Result<usize, SchedulerError> {
        let evicted: i64 = sqlx::query_scalar(EVICT)
            .bind(&*self.name)
            .fetch_one(&self.pool)
            .await
            .map_err(backend("evict"))?;
        Ok(usize::try_from(evicted).unwrap_or(0))
    }
}

#[cfg(not(parking_lot_negative_control = "s-7"))]
impl<T> PgResultMailbox<T> {
    /// The oldest unexpired entry's row, removed by the same statement.
    async fn take_oldest(&self, mailbox_key: &str) -> Result<Option<PgRow>, SchedulerError> {
        sqlx::query(sql::CONSUME)
            .bind(&*self.name)
            .bind(mailbox_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend("consume"))
    }
}

/// `s-7` negative control: read, then delete in a second statement, ignoring
/// whether the delete removed anything.
#[cfg(parking_lot_negative_control = "s-7")]
impl<T> PgResultMailbox<T> {
    async fn take_oldest(&self, mailbox_key: &str) -> Result<Option<PgRow>, SchedulerError> {
        let row = sqlx::query(PEEK)
            .bind(&*self.name)
            .bind(mailbox_key)
            .fetch_optional(&self.pool)
            .await
            .map_err(backend("consume (select)"))?;
        let Some(row) = row else {
            return Ok(None);
        };
        negative_control::gap().await;
        let id: i64 = row.try_get("id").map_err(backend("decode id"))?;
        sqlx::query(sql::DELETE_BY_ID)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(backend("consume (delete)"))?;
        Ok(Some(row))
    }
}

/// Stops the background sweeper started by
/// [`PgResultMailbox::spawn_sweeper`] when dropped.
#[derive(Debug)]
#[must_use = "the sweeper stops when its handle is dropped"]
pub struct SweeperHandle {
    task: JoinHandle<()>,
}

impl Drop for SweeperHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl<T> PgResultMailbox<T>
where
    T: Serialize + DeserializeOwned + Send + Sync + 'static,
{
    /// Start a task that calls [`evict_expired`](ResultMailbox::evict_expired)
    /// every `interval` until the returned handle is dropped.
    ///
    /// The mailbox is correct without the sweeper (reads skip expired entries
    /// and a delivery may reuse an expired key); the sweeper reclaims the
    /// space. Must be called inside a Tokio runtime. A failed pass is logged
    /// and retried on the next tick.
    pub fn spawn_sweeper(&self, interval: Duration) -> SweeperHandle {
        let mailbox = self.clone();
        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                match mailbox.evict_expired().await {
                    Ok(0) => {}
                    Ok(evicted) => {
                        tracing::debug!(evicted, mailbox = mailbox.name(), "mailbox sweep");
                    }
                    Err(error) => {
                        tracing::warn!(%error, mailbox = mailbox.name(), "mailbox sweep failed");
                    }
                }
            }
        });
        SweeperHandle { task }
    }
}

/// Maps a driver error to [`SchedulerError::Backend`] naming the operation.
fn backend<E>(operation: &'static str) -> impl FnOnce(E) -> SchedulerError
where
    E: std::error::Error + Send + Sync + 'static,
{
    move |source| SchedulerError::backend(format!("pg result mailbox: {operation}"), source)
}

/// Decode a row of [`sql::PEEK`] or [`sql::CONSUME`].
fn decode_entry<T: DeserializeOwned>(row: &PgRow) -> Result<MailboxEntry<T>, SchedulerError> {
    let task_id: i64 = row.try_get("task_id").map_err(backend("decode task id"))?;
    let task_id = u64::try_from(task_id)
        .map_err(|_| SchedulerError::internal(format!("negative task id {task_id}")))?;
    let status: serde_json::Value = row.try_get("status").map_err(backend("decode status"))?;
    let status: TaskStatus =
        serde_json::from_value(status).map_err(|e| undecodable(task_id, "status", &e))?;
    let payload: Option<serde_json::Value> =
        row.try_get("payload").map_err(backend("decode payload"))?;
    let payload = payload
        .map(serde_json::from_value::<T>)
        .transpose()
        .map_err(|e| undecodable(task_id, "payload", &e))?;
    let idempotency_key: Option<String> = row
        .try_get("idempotency_key")
        .map_err(backend("decode idempotency key"))?;
    let delivered_at_ms: i64 = row
        .try_get("delivered_at_ms")
        .map_err(backend("decode delivery time"))?;
    Ok(MailboxEntry::new(
        task_id,
        status,
        payload,
        idempotency_key,
        u128::try_from(delivered_at_ms).unwrap_or(0),
    ))
}

fn undecodable(task_id: u64, field: &str, error: &serde_json::Error) -> SchedulerError {
    SchedulerError::internal(format!(
        "mailbox entry for task {task_id} has an undecodable {field}: {error}"
    ))
}
