//! Runtime adapters (native, web/worker, cloud) and API surface.

pub mod api;
pub mod tokio_spawner;

pub use api::{submit_task, TaskStatusResponse, TaskSubmission};
pub use tokio_spawner::TokioSpawner;
