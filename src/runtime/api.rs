//! API-facing request/response models (skeleton).

use serde::{Deserialize, Serialize};

use crate::core::{ResourcePool, ScheduledTask, TaskStatus};
use crate::util::serde::{MailboxKey, Priority, ResourceCost, TaskId};

/// Task submission payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSubmission<P> {
    /// Task identifier.
    pub task_id: TaskId,
    /// Priority.
    pub priority: Priority,
    /// Resource cost.
    pub resource_cost: ResourceCost,
    /// Optional deadline (ms since epoch).
    pub deadline_ms: Option<u128>,
    /// Optional mailbox key.
    pub mailbox_key: Option<MailboxKey>,
    /// Creation time (ms since epoch).
    pub created_at_ms: u128,
    /// Opaque payload.
    pub payload: P,
}

/// Task status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusResponse {
    /// Task identifier.
    pub task_id: TaskId,
    /// Current status.
    pub status: TaskStatus,
    /// Optional reason for failure/drop.
    pub reason: Option<String>,
}

/// Pool snapshot data for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolSnapshot {
    /// Pool identifier.
    pub name: String,
    /// Max units.
    pub max_units: u32,
    /// Max queue depth.
    pub max_queue_depth: usize,
}

/// Health response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Health {
    /// Healthy flag.
    pub ok: bool,
}

/// Submit a task to a pool. Placeholder; caller manages pool lookup.
pub async fn submit_task<P, T, Q, M, E, S>(
    pool: &ResourcePool<P, T, Q, M, E, S>,
    req: TaskSubmission<P>,
    now_ms: u128,
) -> Result<TaskStatus, String>
where
    P: crate::core::TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    Q: crate::core::TaskQueue<P> + Send + 'static,
    M: crate::core::Mailbox<T> + Send + 'static,
    E: crate::core::TaskExecutor<P, T>,
    S: crate::core::Spawn + Clone + Send + 'static,
{
    let meta = crate::core::TaskMetadata {
        id: req.task_id,
        mailbox: req.mailbox_key.clone(),
        priority: req.priority,
        cost: req.resource_cost,
        deadline_ms: req.deadline_ms,
        created_at_ms: req.created_at_ms,
    };
    let task: ScheduledTask<P> = ScheduledTask {
        meta,
        payload: req.payload,
    };
    pool.submit(task, now_ms).await.map_err(|e| e.to_string())
}

/// Build pool listings from config snapshot.
pub fn list_pools(
    cfg: &crate::config::SchedulerConfig,
) -> Vec<PoolSnapshot> {
    cfg.pools
        .iter()
        .map(|(name, pool)| PoolSnapshot {
            name: name.clone(),
            max_units: pool.max_units,
            max_queue_depth: pool.max_queue_depth,
        })
        .collect()
}

/// Return a health payload.
pub fn health() -> Health {
    Health { ok: true }
}
