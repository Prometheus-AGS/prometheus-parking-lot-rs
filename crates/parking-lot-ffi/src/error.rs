//! The error type foreign code sees.

use prometheus_parking_lot_core::SchedulerError;

/// Why an operation failed. Maps to a Kotlin exception hierarchy / a Swift `Error` enum.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
pub enum FfiError {
    /// The lease on a queue entry expired or was already acked or nacked. Another
    /// consumer may now own the task; do not retry the operation with the same handle.
    LeaseLost {
        /// The queue entry whose lease was lost.
        entry_id: u64,
    },
    /// The queue holds its configured maximum of live entries; retry later.
    QueueFull {
        /// Live entries in the queue.
        depth: u64,
        /// The configured maximum.
        capacity: u64,
    },
    /// An argument or the store file was not acceptable (for example a zero lease
    /// duration, or a database written by an incompatible version).
    InvalidConfig {
        /// What was wrong.
        detail: String,
    },
    /// The storage engine failed (disk full, file locked by another process, corruption).
    Backend {
        /// Where and why.
        detail: String,
    },
    /// Anything else, including a failure inside this crate.
    Internal {
        /// What happened.
        detail: String,
    },
}

// The error fields are called `detail`, not `message`: a Kotlin exception class that
// declares `message` collides with `Throwable.message` and the generated file does
// not compile.

impl std::fmt::Display for FfiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LeaseLost { entry_id } => {
                write!(f, "lease on queue entry {entry_id} is no longer held")
            }
            Self::QueueFull { depth, capacity } => {
                write!(f, "queue full: depth {depth}/{capacity}")
            }
            Self::InvalidConfig { detail } => write!(f, "invalid configuration: {detail}"),
            Self::Backend { detail } => write!(f, "backend error: {detail}"),
            Self::Internal { detail } => write!(f, "internal error: {detail}"),
        }
    }
}

impl std::error::Error for FfiError {}

impl From<SchedulerError> for FfiError {
    fn from(error: SchedulerError) -> Self {
        match error {
            SchedulerError::LeaseLost { entry } => Self::LeaseLost { entry_id: entry },
            SchedulerError::QueueFull {
                depth, capacity, ..
            } => Self::QueueFull {
                depth: widen(depth),
                capacity: widen(capacity),
            },
            SchedulerError::InvalidConfig { message } => Self::InvalidConfig { detail: message },
            other @ SchedulerError::Backend { .. } => Self::Backend {
                detail: other.to_string(),
            },
            other => Self::Internal {
                detail: other.to_string(),
            },
        }
    }
}

/// `usize` to the wire's `u64` (never truncates on any supported target).
pub fn widen(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}
