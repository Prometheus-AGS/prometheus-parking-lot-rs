//! Error types for scheduler operations.

use thiserror::Error;

/// Errors produced by scheduler components.
#[derive(Debug, Error)]
pub enum SchedulerError {
    /// Queue is full for the target pool.
    #[error("queue full: {0}")]
    QueueFull(String),
    /// Task would exceed configured capacity.
    #[error("capacity exceeded")]
    CapacityExceeded,
    /// Task deadline has passed.
    #[error("deadline expired")]
    DeadlineExpired,
    /// Backend-specific failure with context.
    #[error("backend error: {0}")]
    Backend(String),
}

/// Application-facing result using anyhow for higher-level contexts.
pub type AppResult<T> = Result<T, anyhow::Error>;
