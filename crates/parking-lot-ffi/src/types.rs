//! The records and enums that cross the binding boundary, and their conversions to
//! and from the core types.
//!
//! Every type also derives `serde`, so a Rust host (the Tauri example) can return
//! them from its own commands without a second set of data types.

use prometheus_parking_lot_core::{
    Delivery, MailboxEntry, MailboxKey, NackAction, Priority, QueueDepth, ResourceCost,
    ResourceKind, TaskError, TaskStatus,
};
use serde::{Deserialize, Serialize};

use crate::error::widen;

/// Where the store lives and how many live entries the queue may hold.
///
/// Mirrors the `redb` section of the facade crate's backend config
/// (`{ "type": "redb", "path": ..., "max_depth": ... }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct StoreConfig {
    /// Path of the database file. Parent directories are created. On Android use a
    /// path under `Context.filesDir`; on iOS under the app's Application Support
    /// directory.
    pub path: String,
    /// Maximum live (ready, delayed or leased) entries; `enqueue` fails with
    /// `QueueFull` beyond it.
    pub max_depth: u64,
}

/// Task priority; higher is leased first, then first-in first-out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum TaskPriority {
    /// Background work.
    Low,
    /// The default.
    Normal,
    /// Ahead of normal work.
    High,
    /// Ahead of everything else.
    Critical,
}

impl From<TaskPriority> for Priority {
    fn from(priority: TaskPriority) -> Self {
        match priority {
            TaskPriority::Low => Self::Low,
            TaskPriority::Normal => Self::Normal,
            TaskPriority::High => Self::High,
            TaskPriority::Critical => Self::Critical,
        }
    }
}

impl From<Priority> for TaskPriority {
    fn from(priority: Priority) -> Self {
        match priority {
            Priority::Low => Self::Low,
            Priority::Normal => Self::Normal,
            Priority::High => Self::High,
            Priority::Critical => Self::Critical,
        }
    }
}

/// A kind of capacity a task consumes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum ResourceClass {
    /// CPU time.
    Cpu,
    /// GPU or NPU memory.
    GpuVram,
    /// Network or disk bandwidth.
    Io,
    /// A task that does not fit one class.
    Mixed,
}

impl ResourceClass {
    pub(crate) const fn to_kind(self) -> ResourceKind {
        match self {
            Self::Cpu => ResourceKind::Cpu,
            Self::GpuVram => ResourceKind::GpuVram,
            Self::Io => ResourceKind::Io,
            Self::Mixed => ResourceKind::Mixed,
        }
    }

    const fn from_kind(kind: ResourceKind) -> Self {
        match kind {
            ResourceKind::Cpu => Self::Cpu,
            ResourceKind::GpuVram => Self::GpuVram,
            ResourceKind::Io => Self::Io,
            // `ResourceKind` is non-exhaustive; a kind added later reads as `Mixed`
            // until this enum learns it.
            _ => Self::Mixed,
        }
    }
}

/// Units of one resource class a task needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ResourceAmount {
    /// Which resource.
    pub resource: ResourceClass,
    /// How many units (zero is dropped).
    pub units: u32,
}

pub fn cost_from(amounts: &[ResourceAmount]) -> ResourceCost {
    amounts
        .iter()
        .map(|a| (a.resource.to_kind(), a.units))
        .collect()
}

pub fn amounts_from(cost: &ResourceCost) -> Vec<ResourceAmount> {
    cost.iter()
        .map(|(kind, units)| ResourceAmount {
            resource: ResourceClass::from_kind(kind),
            units,
        })
        .collect()
}

/// Whose results a task's outcome is delivered to. A mailbox can be read by a client
/// that was offline when the result was produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct MailboxAddress {
    /// Tenant (an app, an account).
    pub tenant: String,
    /// User within the tenant, if the app has users.
    pub user_id: Option<String>,
    /// Session within the user, if results are per session.
    pub session_id: Option<String>,
}

impl From<MailboxAddress> for MailboxKey {
    fn from(address: MailboxAddress) -> Self {
        Self {
            tenant: address.tenant,
            user_id: address.user_id,
            session_id: address.session_id,
        }
    }
}

impl From<MailboxKey> for MailboxAddress {
    fn from(key: MailboxKey) -> Self {
        Self {
            tenant: key.tenant,
            user_id: key.user_id,
            session_id: key.session_id,
        }
    }
}

/// A task to enqueue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct NewTask {
    /// Caller-chosen identifier, echoed on the leased task and on its result.
    pub task_id: u64,
    /// Opaque payload bytes.
    pub payload: Vec<u8>,
    /// Scheduling priority.
    pub priority: TaskPriority,
    /// Resources the task consumes when it runs.
    pub cost: Vec<ResourceAmount>,
    /// Enqueuing a second task with the same key returns the first one's entry
    /// (`EnqueueOutcome.duplicate`) and stores nothing.
    pub idempotency_key: Option<String>,
    /// Where the outcome is delivered; carried back on lease.
    pub mailbox: Option<MailboxAddress>,
    /// Latest start time, as Unix milliseconds.
    pub deadline_ms: Option<u64>,
}

/// What `enqueue` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct EnqueueOutcome {
    /// The queue entry (the new one, or the existing one for a duplicate).
    pub entry_id: u64,
    /// True when the idempotency key was already known and nothing was stored.
    pub duplicate: bool,
}

/// Proof of a lease, required to ack, nack or extend it. Once the lease expires the
/// handle is dead: operations with it fail with `FfiError.LeaseLost`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct LeaseHandle {
    /// The leased queue entry.
    pub entry_id: u64,
    /// Distinguishes this lease from earlier and later ones on the same entry.
    pub fence: u64,
}

/// A task handed to a consumer by `lease`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct LeasedItem {
    /// Pass this to `ack`, `nack` or `extend_lease`.
    pub lease: LeaseHandle,
    /// 1 on first delivery, higher on each redelivery after an expired or nacked lease.
    pub attempt: u32,
    /// The caller-chosen task id.
    pub task_id: u64,
    /// The payload as enqueued.
    pub payload: Vec<u8>,
    /// Scheduling priority.
    pub priority: TaskPriority,
    /// Resources the task consumes.
    pub cost: Vec<ResourceAmount>,
    /// The idempotency key, if any. Use it as the result's key so a redelivery cannot
    /// store a second result.
    pub idempotency_key: Option<String>,
    /// Where to deliver the outcome.
    pub mailbox: Option<MailboxAddress>,
    /// Latest start time, as Unix milliseconds.
    pub deadline_ms: Option<u64>,
    /// When the task was enqueued, as Unix milliseconds.
    pub created_at_ms: u64,
}

/// What `nack` does with the task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum NackDisposition {
    /// Make the task ready again after `delay_ms` (zero: immediately). Its attempt
    /// count goes up on the next lease.
    Requeue {
        /// Delay before the task is leasable again, in milliseconds.
        delay_ms: u64,
    },
    /// Give up on the task; it is kept in the dead-letter set for inspection.
    DeadLetter {
        /// Why.
        reason: String,
    },
}

impl From<NackDisposition> for NackAction {
    fn from(disposition: NackDisposition) -> Self {
        match disposition {
            NackDisposition::Requeue { delay_ms } => {
                Self::requeue_after(std::time::Duration::from_millis(delay_ms))
            }
            NackDisposition::DeadLetter { reason } => Self::dead_letter(reason),
        }
    }
}

/// Entries per state, as of the call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct DepthInfo {
    /// Leasable now.
    pub ready: u64,
    /// Waiting out a requeue delay.
    pub delayed: u64,
    /// Held under a live lease.
    pub leased: u64,
    /// Given up on (not counted against `max_depth`).
    pub dead_lettered: u64,
}

impl DepthInfo {
    /// Entries that count against `max_depth`: ready, delayed and leased.
    #[must_use]
    pub const fn live(&self) -> u64 {
        self.ready + self.delayed + self.leased
    }
}

impl From<QueueDepth> for DepthInfo {
    fn from(depth: QueueDepth) -> Self {
        Self {
            ready: widen(depth.ready),
            delayed: widen(depth.delayed),
            leased: widen(depth.leased),
            dead_lettered: widen(depth.dead_lettered),
        }
    }
}

/// How a task ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum TaskOutcome {
    /// It ran and produced the result payload.
    Completed,
    /// It ran and failed.
    Failed {
        /// What went wrong.
        message: String,
        /// Whether running it again may succeed.
        retryable: bool,
    },
    /// Its deadline passed before it started.
    Expired,
    /// It was discarded without running.
    Dropped {
        /// Why.
        reason: String,
    },
    /// A status this binding has no case for; the text is the core's description.
    Other {
        /// The status, rendered as text.
        description: String,
    },
}

impl From<TaskOutcome> for TaskStatus {
    fn from(outcome: TaskOutcome) -> Self {
        match outcome {
            TaskOutcome::Completed => Self::Completed,
            TaskOutcome::Failed { message, retryable } => {
                Self::Failed(TaskError::Executor { message, retryable })
            }
            TaskOutcome::Expired => Self::Expired,
            TaskOutcome::Dropped { reason } => Self::Dropped(reason),
            TaskOutcome::Other { description } => Self::Dropped(description),
        }
    }
}

impl From<TaskStatus> for TaskOutcome {
    fn from(status: TaskStatus) -> Self {
        match status {
            TaskStatus::Completed => Self::Completed,
            TaskStatus::Failed(error) => Self::Failed {
                retryable: error.is_retryable(),
                message: error.to_string(),
            },
            TaskStatus::Expired => Self::Expired,
            TaskStatus::Dropped(reason) => Self::Dropped { reason },
            other => Self::Other {
                description: format!("{other:?}"),
            },
        }
    }
}

/// A task's outcome to store for a client to collect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ResultDelivery {
    /// Whose mailbox.
    pub mailbox: MailboxAddress,
    /// The task this is the outcome of.
    pub task_id: u64,
    /// How it ended.
    pub outcome: TaskOutcome,
    /// The result payload, if any.
    pub payload: Option<Vec<u8>>,
    /// First write wins: a second delivery with the same key stores nothing and
    /// reports `Duplicate`. Use the task's idempotency key. The key is remembered
    /// even after the result was collected.
    pub idempotency_key: Option<String>,
    /// How long the result stays collectable, in milliseconds; forever when absent.
    pub ttl_ms: Option<u64>,
}

impl ResultDelivery {
    pub(crate) fn into_delivery(self) -> Delivery<crate::payload::Payload> {
        let mut delivery = Delivery::new(
            self.mailbox.into(),
            self.task_id,
            self.outcome.into(),
            self.payload.map(crate::payload::Payload),
        );
        if let Some(key) = self.idempotency_key {
            delivery = delivery.with_idempotency_key(key);
        }
        if let Some(ttl_ms) = self.ttl_ms {
            delivery = delivery.with_ttl(std::time::Duration::from_millis(ttl_ms));
        }
        delivery
    }
}

/// What `deliver_result` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
pub enum DeliveryResult {
    /// The result was stored.
    Stored,
    /// The idempotency key was already known; nothing was stored.
    Duplicate,
}

/// A stored result, as read by `consume_result` or `peek_result`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct ResultEntry {
    /// The task this is the outcome of.
    pub task_id: u64,
    /// How it ended.
    pub outcome: TaskOutcome,
    /// The result payload, if any.
    pub payload: Option<Vec<u8>>,
    /// The key the delivery carried.
    pub idempotency_key: Option<String>,
    /// When it was stored, as Unix milliseconds.
    pub delivered_at_ms: u64,
}

impl From<MailboxEntry<crate::payload::Payload>> for ResultEntry {
    fn from(entry: MailboxEntry<crate::payload::Payload>) -> Self {
        Self {
            task_id: entry.task_id,
            outcome: entry.status.into(),
            payload: entry.payload.map(|p| p.0),
            idempotency_key: entry.idempotency_key,
            delivered_at_ms: u64::try_from(entry.delivered_at_ms).unwrap_or(u64::MAX),
        }
    }
}
