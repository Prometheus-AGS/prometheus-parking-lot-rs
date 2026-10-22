//! Runtime adapters (native, web/worker, cloud) and API surface.
//!
//! The tokio spawner lives in `prometheus-parking-lot-runtime-tokio` and is
//! re-exported here; the API-surface skeleton in `api` wires the facade's
//! configuration to the pool and stays in the facade (ADR 0001).

pub mod api;
pub use prometheus_parking_lot_runtime_tokio::tokio_spawner;

pub use api::{TaskStatusResponse, TaskSubmission, submit_task};
pub use tokio_spawner::TokioSpawner;
