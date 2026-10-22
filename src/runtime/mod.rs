//! Runtime adapters (native, web/worker, cloud) and API surface.

pub mod api;
pub mod tokio_spawner;

pub use api::{TaskStatusResponse, TaskSubmission, submit_task};
pub use tokio_spawner::TokioSpawner;
