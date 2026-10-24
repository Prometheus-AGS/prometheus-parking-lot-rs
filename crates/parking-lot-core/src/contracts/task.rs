//! Task metadata, scheduled-task envelope, lifecycle status, submission
//! sequencing and pool limits.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::contracts::error::TaskError;
use crate::contracts::types::{MailboxKey, Priority, ResourceCost, TaskId};

/// Status of a task in the scheduler lifecycle, as recorded in a mailbox.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub enum TaskStatus {
    /// Task is queued waiting for capacity.
    Queued,
    /// Task is running.
    Running,
    /// Task finished successfully.
    Completed,
    /// Task ran (or was due to run) and failed; the error says how: the
    /// executor returned an error, panicked, exceeded its budget or was
    /// cancelled.
    Failed(TaskError),
    /// Task's deadline passed before it could start; it never ran.
    Expired,
    /// Task was rejected or dropped.
    Dropped(String),
}

/// Metadata describing a scheduled task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskMetadata {
    /// Task identifier, supplied by the caller; the pool does not generate or deduplicate it, so it can repeat.
    pub id: TaskId,
    /// Tenant/user/session identification for isolation and mailbox routing.
    pub mailbox: Option<MailboxKey>,
    /// Priority used for queue ordering.
    pub priority: Priority,
    /// Resource cost for capacity accounting.
    pub cost: ResourceCost,
    /// Absolute deadline in milliseconds since epoch. The task is expired
    /// once the time is past it (`now_ms > deadline_ms`); at the deadline
    /// itself it is still live.
    pub deadline_ms: Option<u128>,
    /// Creation timestamp in milliseconds since epoch.
    pub created_at_ms: u128,
    /// Caller-supplied stable idempotency key for at-least-once deduplication
    /// and replay across redeliveries.
    ///
    /// When `Some`, this key survives cancellation and redelivery so callers
    /// can detect and discard duplicate executions. Set this to a stable,
    /// content-derived string (e.g. a hash of the inference request) for any
    /// task you want to make safely replayable.
    ///
    /// When `None` the task is treated as unique — no deduplication is
    /// performed and cancelled copies are not automatically matched to
    /// redeliveries. The in-memory backend provides no cross-restart
    /// durability regardless.
    #[serde(default)]
    pub idempotency_key: Option<String>,
    /// Submission sequence number: the FIFO key within a priority.
    ///
    /// A pool stamps this from its [`SubmissionSequence`] when the task is
    /// submitted, overwriting any caller value, so equal-priority tasks are
    /// dispatched in submission order even when they share a millisecond
    /// timestamp. Callers set `0`. It travels with the task, so a task that is
    /// dequeued and put back keeps its place.
    #[serde(default)]
    pub sequence: u64,
}

/// A schedulable task with metadata and payload.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "P: serde::Serialize"))]
#[serde(bound(deserialize = "P: serde::de::DeserializeOwned"))]
pub struct ScheduledTask<P> {
    /// Metadata driving scheduling decisions.
    pub meta: TaskMetadata,
    /// Task payload supplied by caller.
    pub payload: P,
}

/// Monotonic source of [`TaskMetadata::sequence`] numbers.
///
/// Each pool owns one. Numbers start at 1 and strictly increase, so `0`
/// means "not yet sequenced".
#[derive(Debug)]
pub struct SubmissionSequence {
    next: AtomicU64,
}

impl SubmissionSequence {
    /// A sequence whose first number is 1.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            next: AtomicU64::new(1),
        }
    }

    /// Take the next number. Numbers taken by one thread strictly increase;
    /// across threads each number is handed out once.
    pub fn next(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }
}

impl Default for SubmissionSequence {
    fn default() -> Self {
        Self::new()
    }
}

/// Configuration values for capacity enforcement.
#[derive(Debug, Clone)]
pub struct PoolLimits {
    /// Maximum concurrent resource units.
    pub max_units: u32,
    /// Maximum queued tasks.
    pub max_queue_depth: usize,
    /// Default execution budget for a task without a nearer deadline.
    pub default_timeout: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_sequence_starts_at_one_and_increases() {
        let seq = SubmissionSequence::new();
        assert_eq!(seq.next(), 1);
        assert_eq!(seq.next(), 2);
        assert_eq!(seq.next(), 3);
    }

    #[test]
    fn metadata_without_sequence_deserializes_as_unsequenced() {
        let json = r#"{"id":7,"mailbox":null,"priority":"normal",
            "cost":{"kind":"cpu","units":1},"deadline_ms":null,"created_at_ms":0}"#;
        let meta: TaskMetadata = serde_json::from_str(json).expect("0.2 metadata");
        assert_eq!(meta.sequence, 0);
        assert_eq!(meta.cost.total_units(), 1);
    }
}
