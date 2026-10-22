//! Postgres-backed mailbox adapter using `SeaORM`.
//!
//! This implementation provides persistent result storage with tenant isolation
//! and optional HTTP webhook notifications.

#![cfg_attr(
    feature = "postgres",
    expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "u128 epoch-ms and u64 limits fit i64/usize in practice; this SeaORM backend is replaced by the raw-sqlx lease queue and mailbox in cr-26/cr-27, which own checked conversions"
    )
)]

#[cfg(feature = "postgres")]
use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect,
};

#[cfg(feature = "postgres")]
use serde::{Serialize, de::DeserializeOwned};

use crate::core::{Mailbox, SchedulerError, TaskStatus};
use crate::util::serde::MailboxKey;

#[cfg(feature = "postgres")]
use crate::infra::entities::mailbox_message::{self, Entity as MailboxMessage};

/// Postgres mailbox adapter.
pub struct PostgresMailbox<P> {
    #[cfg(feature = "postgres")]
    db: DatabaseConnection,
    _marker: std::marker::PhantomData<P>,
}

impl<P> PostgresMailbox<P> {
    /// Create a new adapter with database connection.
    #[cfg(feature = "postgres")]
    #[must_use]
    pub const fn new(db: DatabaseConnection) -> Self {
        Self {
            db,
            _marker: std::marker::PhantomData,
        }
    }

    /// Create a new adapter without database connection (stub for non-postgres builds).
    #[cfg(not(feature = "postgres"))]
    #[must_use]
    #[expect(
        clippy::new_without_default,
        reason = "stub constructor exists only in non-postgres builds; the backend is rewritten in cr-27"
    )]
    pub const fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    /// Migration statements for mailbox storage.
    #[must_use]
    pub const fn migrations() -> &'static [&'static str] {
        &[r"
CREATE TABLE IF NOT EXISTS pl_mailbox_messages (
    id BIGSERIAL PRIMARY KEY,
    tenant TEXT NOT NULL,
    user_id TEXT,
    session_id TEXT,
    task_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms BIGINT NOT NULL,
    status TEXT NOT NULL,
    payload JSONB
);
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_lookup
    ON pl_mailbox_messages (tenant, user_id, session_id, created_at_ms);
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_task_id
    ON pl_mailbox_messages (task_id)
    WHERE task_id IS NOT NULL;
"]
    }

    /// Convert `TaskStatus` to string representation.
    #[cfg(feature = "postgres")]
    fn status_to_string(status: &TaskStatus) -> String {
        match status {
            TaskStatus::Queued => "Queued".to_string(),
            TaskStatus::Running => "Running".to_string(),
            TaskStatus::Completed => "Completed".to_string(),
            TaskStatus::Failed(msg) => format!("Failed: {msg}"),
            TaskStatus::Expired => "Expired".to_string(),
            TaskStatus::Dropped(msg) => format!("Dropped: {msg}"),
        }
    }

    /// Fetch messages from mailbox.
    ///
    /// # Errors
    ///
    /// Returns a backend error when the query fails or a stored payload cannot
    /// be deserialized.
    #[cfg(feature = "postgres")]
    #[expect(
        clippy::future_not_send,
        reason = "Send-ness depends on P; the async lease contract (cr-25) and cr-27 rewrite own the bounds"
    )]
    pub async fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Result<Vec<MailboxMessageDto<P>>, SchedulerError>
    where
        P: DeserializeOwned,
    {
        let mut query =
            MailboxMessage::find().filter(mailbox_message::Column::Tenant.eq(&key.tenant));

        if let Some(user_id) = &key.user_id {
            query = query.filter(mailbox_message::Column::UserId.eq(user_id));
        } else {
            query = query.filter(mailbox_message::Column::UserId.is_null());
        }

        if let Some(session_id) = &key.session_id {
            query = query.filter(mailbox_message::Column::SessionId.eq(session_id));
        } else {
            query = query.filter(mailbox_message::Column::SessionId.is_null());
        }

        if let Some(since) = since_ms {
            query = query.filter(mailbox_message::Column::CreatedAtMs.gte(since as i64));
        }

        let messages = query
            .order_by_asc(mailbox_message::Column::CreatedAtMs)
            .limit(limit as u64)
            .all(&self.db)
            .await
            .map_err(|e| SchedulerError::backend("fetch messages failed", e))?;

        let mut results = Vec::new();
        for msg in messages {
            let payload =
                if let Some(payload_json) = msg.payload {
                    Some(serde_json::from_value(payload_json).map_err(|e| {
                        SchedulerError::backend("payload deserialization failed", e)
                    })?)
                } else {
                    None
                };

            results.push(MailboxMessageDto {
                id: msg.id,
                task_id: msg.task_id.map(|id| id as u64),
                created_at_ms: msg.created_at_ms as u128,
                status: msg.status,
                payload,
            });
        }

        Ok(results)
    }

    /// Prune old messages based on retention period.
    ///
    /// # Errors
    ///
    /// Returns a backend error when the delete fails.
    #[cfg(feature = "postgres")]
    #[expect(
        clippy::future_not_send,
        reason = "Send-ness depends on P; the async lease contract (cr-25) and cr-27 rewrite own the bounds"
    )]
    pub async fn prune_old(&self, older_than_ms: u128) -> Result<usize, SchedulerError> {
        let result = MailboxMessage::delete_many()
            .filter(mailbox_message::Column::CreatedAtMs.lt(older_than_ms as i64))
            .exec(&self.db)
            .await
            .map_err(|e| SchedulerError::backend("prune operation failed", e))?;

        Ok(result.rows_affected as usize)
    }
}

/// DTO for fetched mailbox messages.
#[derive(Debug, Clone)]
pub struct MailboxMessageDto<P> {
    /// Message ID.
    pub id: i64,
    /// Task ID (if available).
    pub task_id: Option<u64>,
    /// Creation timestamp in milliseconds.
    pub created_at_ms: u128,
    /// Status string.
    pub status: String,
    /// Payload (if available).
    pub payload: Option<P>,
}

#[cfg(feature = "postgres")]
impl<P> Mailbox<P> for PostgresMailbox<P>
where
    P: Serialize + DeserializeOwned,
{
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<P>,
    ) -> Result<(), SchedulerError> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| SchedulerError::internal("no tokio runtime available"))?;

        rt.block_on(async {
            let now = crate::util::clock::now_ms();
            let status_str = Self::status_to_string(&status);

            let payload_json = if let Some(p) = payload {
                Some(
                    serde_json::to_value(p)
                        .map_err(|e| SchedulerError::backend("payload serialization failed", e))?,
                )
            } else {
                None
            };

            let created_at = chrono::DateTime::from_timestamp(
                (now / 1000) as i64,
                ((now % 1000) * 1_000_000) as u32,
            )
            .unwrap_or_else(chrono::Utc::now)
            .into();

            let active_model = mailbox_message::ActiveModel {
                id: ActiveValue::NotSet,
                tenant: ActiveValue::Set(key.tenant.clone()),
                user_id: ActiveValue::Set(key.user_id.clone()),
                session_id: ActiveValue::Set(key.session_id.clone()),
                task_id: ActiveValue::NotSet,
                created_at: ActiveValue::Set(created_at),
                created_at_ms: ActiveValue::Set(now as i64),
                status: ActiveValue::Set(status_str),
                payload: ActiveValue::Set(payload_json),
            };

            active_model
                .insert(&self.db)
                .await
                .map_err(|e| SchedulerError::backend("mailbox insert failed", e))?;

            Ok(())
        })
    }
}

#[cfg(not(feature = "postgres"))]
impl<P> Mailbox<P> for PostgresMailbox<P> {
    fn deliver(
        &mut self,
        _key: &MailboxKey,
        _status: TaskStatus,
        _payload: Option<P>,
    ) -> Result<(), SchedulerError> {
        Err(SchedulerError::internal(
            "postgres mailbox not available (feature disabled)",
        ))
    }
}
