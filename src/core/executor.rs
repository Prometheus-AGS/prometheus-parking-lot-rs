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

/// Executor trait for worker pools that does NOT require serialization on results.
/// 
/// This is the primary executor trait for `WorkerPool`. Unlike `TaskExecutor`,
/// this trait allows result types that cannot be serialized, such as:
/// - Streaming channels (`flume::Receiver`, `tokio::sync::mpsc::Receiver`)
/// - Complex types with non-serializable fields
/// - Types containing file handles or network connections
/// 
/// # Example
/// 
/// ```rust,ignore
/// use async_trait::async_trait;
/// use prometheus_parking_lot::core::{WorkerExecutor, TaskMetadata};
/// 
/// #[derive(Clone)]
/// struct LlmExecutor;
/// 
/// struct InferenceJob {
///     prompt: String,
///     is_streaming: bool,
/// }
/// 
/// enum InferenceResult {
///     Completion { text: String },
///     Streaming { rx: flume::Receiver<String> },  // Non-serializable!
/// }
/// 
/// #[async_trait]
/// impl WorkerExecutor<InferenceJob, InferenceResult> for LlmExecutor {
///     async fn execute(&self, job: InferenceJob, _meta: TaskMetadata) -> InferenceResult {
///         if job.is_streaming {
///             let (tx, rx) = flume::unbounded();
///             // Spawn streaming task...
///             InferenceResult::Streaming { rx }
///         } else {
///             InferenceResult::Completion { text: "Hello".into() }
///         }
///     }
/// }
/// ```
#[async_trait]
pub trait WorkerExecutor<P, R>: Send + Sync + Clone + 'static
where
    P: Send + 'static,
    R: Send + 'static,  // NO Serialize requirement - supports channels, etc.
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
    /// The result of task execution. This result does NOT need to be serializable,
    /// allowing for streaming channels and other non-serializable types.
    /// 
    /// # Threading
    /// 
    /// On native platforms, this method is called from a dedicated worker thread
    /// with its own single-threaded tokio runtime. This ensures CPU/GPU-bound
    /// work does not block the main async runtime.
    async fn execute(&self, payload: P, meta: TaskMetadata) -> R;
}
