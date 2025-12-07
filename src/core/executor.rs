//! Task execution traits and payload abstraction.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::TaskMetadata;

/// Marker trait for serializable task payloads.
/// 
/// All task payloads must be Send + Sync for cross-thread execution,
/// and Serialize + Deserialize for persistence in queue backends.
pub trait TaskPayload: Send + Sync + Serialize + for<'de> Deserialize<'de> + 'static {}

/// Blanket implementation: any type meeting the requirements is a TaskPayload.
impl<T> TaskPayload for T where T: Send + Sync + Serialize + for<'de> Deserialize<'de> + 'static {}

/// Abstraction for executing a task payload and producing a result.
/// 
/// The executor is responsible for the actual business logic of running a task.
/// It receives the payload `P` and metadata, then returns a result `T`.
/// 
/// # Example
/// 
/// ```rust,ignore
/// use async_trait::async_trait;
/// use prometheus_parking_lot::core::{TaskExecutor, TaskMetadata};
/// 
/// #[derive(Clone)]
/// struct LlmExecutor;
/// 
/// #[derive(serde::Serialize, serde::Deserialize)]
/// struct LlmJob {
///     model: String,
///     prompt: String,
/// }
/// 
/// #[async_trait]
/// impl TaskExecutor<LlmJob, String> for LlmExecutor {
///     async fn execute(&self, payload: LlmJob, _meta: TaskMetadata) -> String {
///         format!("Result from {}: {}", payload.model, payload.prompt)
///     }
/// }
/// ```
#[async_trait]
pub trait TaskExecutor<P, T>: Send + Sync + Clone + 'static
where
    P: TaskPayload,
    T: Send + Sync + Serialize + for<'de> Deserialize<'de> + 'static,
{
    /// Execute a task payload and return the result.
    /// 
    /// # Arguments
    /// 
    /// * `payload` - The task payload to execute
    /// * `meta` - Task metadata including ID, priority, cost, etc.
    /// 
    /// # Returns
    /// 
    /// The result of task execution. This will be delivered to the mailbox
    /// if a mailbox key is present in the task metadata.
    async fn execute(&self, payload: P, meta: TaskMetadata) -> T;
}
