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
//!   entry in one `INSERT ... ON CONFLICT` statement. A short transaction
//!   retains its row lock through duplicate-status lookup. The key is remembered
//!   with its original full status after consume, until the original TTL elapses.
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

mod migration;
mod sql;

use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, LegacyDeliveryStatus, LegacyIdempotencyOwner, MailboxEntry,
    MailboxKey, ResultMailbox, SchedulerError, TaskStatus,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::{PgPool, Row, postgres::PgRow};
use tokio::task::JoinHandle;

use self::sql::{DELIVER_KEYED, DELIVER_PLAIN, EVICT, PEEK};

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

    /// Create the address-scoped schema, or convert a legacy mailbox when every
    /// active fence has provable ownership and original status. Safe to repeat.
    ///
    /// Consumed legacy fences require [`Self::migrate_with_idempotency_metadata`].
    /// Stop old readers/writers and retain an untouched backup before conversion.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the migration fails.
    pub async fn migrate(pool: &PgPool) -> Result<(), SchedulerError> {
        Self::migrate_with_idempotency_owners(pool, &[]).await
    }

    /// Atomically migrate with authoritative ownership evidence for legacy keys.
    ///
    /// Each outer string is the exact mailbox name. An active legacy key whose
    /// retained result does not uniquely match its original expiry requires an
    /// assignment. Duplicate, unknown and contradictory assignments fail. No TTL
    /// is restarted; legacy data remains immutable provenance after conversion.
    /// This owner-only API supplies no status evidence: unknown consumed status
    /// requires [`Self::migrate_with_idempotency_metadata`].
    /// Old SQL writers are rejected by database triggers. Rollback requires the
    /// untouched pre-upgrade backup with every new writer stopped (ADR 0006).
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] on database failure, or
    /// [`SchedulerError::Internal`] on invalid or
    /// insufficient ownership evidence. Refusal rolls back logical changes.
    pub async fn migrate_with_idempotency_owners(
        pool: &PgPool,
        owners: &[(String, LegacyIdempotencyOwner)],
    ) -> Result<(), SchedulerError> {
        Self::migrate_with_idempotency_metadata(pool, owners, &[]).await
    }

    /// Atomically resolve ownership and original status, then convert to protocol 3.
    ///
    /// Status evidence is keyed by the exact mailbox name, complete address and
    /// caller key. Unknown consumed status requires authoritative evidence; it is
    /// never inferred from a retry. Retained proof must match the original native
    /// expiry generation. Direct 0.3 conversion and intermediate v2 conversion
    /// both refuse atomically if owner/status evidence is insufficient. Successful
    /// evidence replay only validates immutable proof and never renews a fence.
    /// Old v1/v2 writers must be stopped; rollback restores the untouched backup
    /// with all new writers stopped (ADRs 0006 and 0007).
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] on storage failure; [`SchedulerError::Internal`]
    /// on missing, duplicate, unknown or contradictory evidence.
    pub async fn migrate_with_idempotency_metadata(
        pool: &PgPool,
        owners: &[(String, LegacyIdempotencyOwner)],
        statuses: &[(String, LegacyDeliveryStatus)],
    ) -> Result<(), SchedulerError> {
        migration::migrate(pool, owners, statuses).await
    }
}

/// The stored form of a mailbox key: its JSON text, which is canonical because
/// the field order of [`MailboxKey`] is fixed.
fn key_text(key: &MailboxKey) -> Result<String, SchedulerError> {
    serde_json::to_string(key).map_err(backend("encode mailbox key"))
}

/// The OA-06 negative control restores the 0.3 mailbox-global fence scope.
#[cfg(parking_lot_negative_control = "oa-06")]
const fn idempotency_scope(_mailbox_key: &str) -> &str {
    ""
}

#[cfg(not(parking_lot_negative_control = "oa-06"))]
const fn idempotency_scope(mailbox_key: &str) -> &str {
    mailbox_key
}

impl<T> ResultMailbox<T> for PgResultMailbox<T>
where
    T: Serialize + DeserializeOwned + Send + Sync,
{
    /// Atomically claim the key and store the entry, retaining original status
    /// through a duplicate lookup in the same short transaction. Dropping the
    /// future may or may not have stored the entry; a retry with the same key
    /// resolves to `Duplicate` with the original status if it did.
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

        if delivery.idempotency_key.is_some() {
            return self
                .deliver_keyed(&delivery, &mailbox_key, task_id, status, payload)
                .await;
        }
        sqlx::query_scalar::<_, i64>(DELIVER_PLAIN)
            .bind(&*self.name)
            .bind(&mailbox_key)
            .bind(task_id)
            .bind(status)
            .bind(payload)
            .bind(ttl_secs)
            .fetch_one(&self.pool)
            .await
            .map_err(backend("deliver"))?;
        Ok(DeliveryOutcome::Stored)
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

impl<T: Sync> PgResultMailbox<T> {
    /// The claim lock survives the duplicate lookup's new statement snapshot.
    async fn deliver_keyed(
        &self,
        delivery: &Delivery<T>,
        mailbox_key: &str,
        task_id: i64,
        status: serde_json::Value,
        payload: Option<serde_json::Value>,
    ) -> Result<DeliveryOutcome, SchedulerError> {
        let mut tx = self.pool.begin().await.map_err(backend("begin delivery"))?;
        let stored = sqlx::query_scalar::<_, i64>(DELIVER_KEYED)
            .bind(&*self.name)
            .bind(mailbox_key)
            .bind(task_id)
            .bind(status)
            .bind(payload)
            .bind(delivery.ttl.map(|ttl| ttl.as_secs_f64()))
            .bind(&delivery.idempotency_key)
            .bind(idempotency_scope(mailbox_key))
            .fetch_optional(&mut *tx)
            .await
            .map_err(backend("deliver"))?;
        let outcome = if stored.is_some() {
            DeliveryOutcome::Stored
        } else {
            let status: serde_json::Value = sqlx::query_scalar(sql::DUPLICATE_STATUS)
                .bind(&*self.name)
                .bind(idempotency_scope(mailbox_key))
                .bind(&delivery.idempotency_key)
                .fetch_one(&mut *tx)
                .await
                .map_err(backend("read duplicate status"))?;
            let status: TaskStatus =
                serde_json::from_value(status).map_err(backend("decode duplicate status"))?;
            duplicate_outcome(status, &delivery.status)
        };
        tx.commit().await.map_err(backend("commit delivery"))?;
        Ok(outcome)
    }
}

#[cfg(not(parking_lot_negative_control = "oa-07"))]
const fn duplicate_outcome(status: TaskStatus, _retry: &TaskStatus) -> DeliveryOutcome {
    DeliveryOutcome::Duplicate { status }
}

/// OA-07 control restores the defect: local retry status replaces stored truth.
#[cfg(parking_lot_negative_control = "oa-07")]
fn duplicate_outcome(_status: TaskStatus, retry: &TaskStatus) -> DeliveryOutcome {
    DeliveryOutcome::Duplicate {
        status: retry.clone(),
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
