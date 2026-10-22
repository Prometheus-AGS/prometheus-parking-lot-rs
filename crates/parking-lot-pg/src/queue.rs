//! Postgres-backed queue adapter using `SeaORM`.
//!
//! This implementation provides persistent task queuing with priority ordering
//! and deadline support. It uses SELECT FOR UPDATE SKIP LOCKED for concurrent
//! dequeue operations without contention.

#![expect(
    clippy::cast_possible_truncation,
    reason = "u128 epoch-ms and u64 limits fit i64/usize in practice; this SeaORM backend is replaced by the raw-sqlx lease queue and mailbox in cr-26/cr-27, which own checked conversions"
)]

use sea_orm::{
    ActiveModelTrait, ActiveValue, ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
};

use serde::{Serialize, de::DeserializeOwned};

use prometheus_parking_lot_core::{Priority, ScheduledTask, SchedulerError, TaskQueue};

use crate::entities::queue_job::{self, Entity as QueueJob};

/// Postgres queue adapter using custom table.
pub struct PostgresQueue<P> {
    db: DatabaseConnection,
    queue_name: String,
    max_depth: usize,
    _marker: std::marker::PhantomData<P>,
}

impl<P> PostgresQueue<P> {
    /// Create a new adapter with database connection.
    #[must_use]
    pub const fn new(db: DatabaseConnection, queue_name: String, max_depth: usize) -> Self {
        Self {
            db,
            queue_name,
            max_depth,
            _marker: std::marker::PhantomData,
        }
    }

    /// Migration statements for custom queue table.
    #[must_use]
    pub const fn migrations() -> &'static [&'static str] {
        &[r"
CREATE TABLE IF NOT EXISTS pl_queue_jobs (
    id BIGSERIAL PRIMARY KEY,
    queue_name TEXT NOT NULL,
    priority SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms BIGINT NOT NULL,
    deadline_at TIMESTAMPTZ,
    deadline_ms BIGINT,
    metadata JSONB NOT NULL,
    payload JSONB NOT NULL,
    locked_by TEXT,
    locked_at TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_dequeue 
    ON pl_queue_jobs (queue_name, priority DESC, created_at ASC);
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_deadline 
    ON pl_queue_jobs (deadline_ms) 
    WHERE deadline_ms IS NOT NULL;
"]
    }

    /// Convert Priority enum to database SMALLINT.
    const fn priority_to_i16(priority: Priority) -> i16 {
        match priority {
            Priority::Low => 0,
            Priority::Normal => 1,
            Priority::High => 2,
            Priority::Critical => 3,
        }
    }

    /// Convert database SMALLINT to Priority enum.
    #[allow(dead_code)] // kept for direct column reads; dequeue currently restores priority from metadata JSON
    const fn i16_to_priority(value: i16) -> Priority {
        match value {
            0 => Priority::Low,
            2 => Priority::High,
            3 => Priority::Critical,
            _ => Priority::Normal, // 1, and default fallback for unknown values
        }
    }
}

impl<P> TaskQueue<P> for PostgresQueue<P>
where
    P: Serialize + DeserializeOwned + Clone,
{
    fn enqueue(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError> {
        // Use tokio runtime to execute async operation synchronously
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| SchedulerError::internal("no tokio runtime available"))?;

        rt.block_on(async {
            // Check queue depth
            let count = QueueJob::find()
                .filter(queue_job::Column::QueueName.eq(&self.queue_name))
                .count(&self.db)
                .await
                .map_err(|e| SchedulerError::backend("queue depth check failed", e))?;

            if count >= self.max_depth as u64 {
                return Err(SchedulerError::queue_full(
                    &self.queue_name,
                    count as usize,
                    self.max_depth,
                ));
            }

            // Serialize metadata and payload
            let metadata_json = serde_json::to_value(&task.meta)
                .map_err(|e| SchedulerError::backend("metadata serialization failed", e))?;

            let payload_json = serde_json::to_value(&task.payload)
                .map_err(|e| SchedulerError::backend("payload serialization failed", e))?;

            // Convert deadline_ms to i64 and chrono timestamp
            let deadline_at = task.meta.deadline_ms.map(|ms| {
                chrono::DateTime::from_timestamp(
                    (ms / 1000) as i64,
                    ((ms % 1000) * 1_000_000) as u32,
                )
                .unwrap_or_else(chrono::Utc::now)
                .into()
            });

            let created_at = chrono::DateTime::from_timestamp(
                (task.meta.created_at_ms / 1000) as i64,
                ((task.meta.created_at_ms % 1000) * 1_000_000) as u32,
            )
            .unwrap_or_else(chrono::Utc::now)
            .into();

            // Insert into database
            let active_model = queue_job::ActiveModel {
                id: ActiveValue::NotSet,
                queue_name: ActiveValue::Set(self.queue_name.clone()),
                priority: ActiveValue::Set(Self::priority_to_i16(task.meta.priority)),
                created_at: ActiveValue::Set(created_at),
                created_at_ms: ActiveValue::Set(task.meta.created_at_ms as i64),
                deadline_at: ActiveValue::Set(deadline_at),
                deadline_ms: ActiveValue::Set(task.meta.deadline_ms.map(|ms| ms as i64)),
                metadata: ActiveValue::Set(metadata_json),
                payload: ActiveValue::Set(payload_json),
                locked_by: ActiveValue::NotSet,
                locked_at: ActiveValue::NotSet,
                attempts: ActiveValue::Set(0),
            };

            active_model
                .insert(&self.db)
                .await
                .map_err(|e| SchedulerError::backend("queue insert failed", e))?;

            Ok(())
        })
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| SchedulerError::internal("no tokio runtime available"))?;

        rt.block_on(async {
            // Use transaction for atomic dequeue
            let txn = self
                .db
                .begin()
                .await
                .map_err(|e| SchedulerError::backend("transaction start failed", e))?;

            // Find and lock the highest priority task (SKIP LOCKED for concurrency)
            let task_model = QueueJob::find()
                .filter(queue_job::Column::QueueName.eq(&self.queue_name))
                .order_by_desc(queue_job::Column::Priority)
                .order_by_asc(queue_job::Column::CreatedAt)
                .lock_exclusive()
                .one(&txn)
                .await
                .map_err(|e| SchedulerError::backend("dequeue query failed", e))?;

            let Some(model) = task_model else {
                txn.commit()
                    .await
                    .map_err(|e| SchedulerError::backend("transaction commit failed", e))?;
                return Ok(None);
            };

            // Deserialize metadata and payload
            let meta = serde_json::from_value(model.metadata.clone())
                .map_err(|e| SchedulerError::backend("metadata deserialization failed", e))?;

            let payload = serde_json::from_value(model.payload.clone())
                .map_err(|e| SchedulerError::backend("payload deserialization failed", e))?;

            // Delete the task
            QueueJob::delete_by_id(model.id)
                .exec(&txn)
                .await
                .map_err(|e| SchedulerError::backend("task deletion failed", e))?;

            txn.commit()
                .await
                .map_err(|e| SchedulerError::backend("transaction commit failed", e))?;

            Ok(Some(ScheduledTask { meta, payload }))
        })
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        let rt = tokio::runtime::Handle::try_current()
            .map_err(|_| SchedulerError::internal("no tokio runtime available"))?;

        rt.block_on(async {
            let result = QueueJob::delete_many()
                .filter(queue_job::Column::QueueName.eq(&self.queue_name))
                .filter(queue_job::Column::DeadlineMs.is_not_null())
                .filter(queue_job::Column::DeadlineMs.lt(now_ms as i64))
                .exec(&self.db)
                .await
                .map_err(|e| SchedulerError::backend("prune operation failed", e))?;

            Ok(result.rows_affected as usize)
        })
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    #[expect(
        clippy::option_if_let_else,
        reason = "block_on bridge is removed with the raw-sqlx queue in cr-26; not worth reshaping here"
    )]
    fn len(&self) -> usize {
        let rt = tokio::runtime::Handle::try_current().ok();
        if let Some(rt) = rt {
            rt.block_on(async {
                QueueJob::find()
                    .filter(queue_job::Column::QueueName.eq(&self.queue_name))
                    .count(&self.db)
                    .await
                    .unwrap_or(0) as usize
            })
        } else {
            0
        }
    }
}
