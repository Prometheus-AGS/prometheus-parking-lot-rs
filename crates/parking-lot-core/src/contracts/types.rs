//! Serialization-friendly core types and helpers.

use serde::{Deserialize, Serialize};

/// Unique task identifier.
pub type TaskId = u64;

/// Task priority for ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Lowest urgency.
    Low,
    /// Default urgency.
    Normal,
    /// Elevated urgency.
    High,
    /// Highest urgency.
    Critical,
}

/// Resource kind used for capacity accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    /// CPU-bound work.
    Cpu,
    /// GPU VRAM-bound work.
    GpuVram,
    /// I/O-bound work.
    Io,
    /// Composite or custom resource.
    Mixed,
}

/// Resource cost expressed in capacity units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceCost {
    /// Kind of resource being consumed.
    pub kind: ResourceKind,
    /// Number of units required.
    pub units: u32,
}

/// Mailbox routing key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailboxKey {
    /// Tenant identifier.
    pub tenant: String,
    /// Optional user identifier.
    pub user_id: Option<String>,
    /// Optional session identifier.
    pub session_id: Option<String>,
}
