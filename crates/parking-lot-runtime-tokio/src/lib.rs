//! # Prometheus Parking Lot - tokio runtime
//!
//! The tokio-based runtime layer of the `prometheus_parking_lot` scheduler:
//! the async [`ResourcePool`] and its crash-durable mode
//! [`DurableResourcePool`] (cr-49), the native [`WorkerPool`] (dedicated OS
//! threads), graceful shutdown, the cancellation channel and the tokio
//! [`TokioSpawner`].
//!
//! Everything here builds on the runtime-free contracts in
//! `prometheus-parking-lot-core`. Most applications use the
//! `prometheus_parking_lot` facade, which re-exports this crate behind its
//! default `tokio-runtime` feature.

pub mod cancellation;
mod contain;
pub mod durable_pool;
pub mod resource_pool;
pub mod shutdown;
pub mod tokio_spawner;
#[cfg(not(target_arch = "wasm32"))]
pub mod worker_pool;

pub use cancellation::{
    CancelReason, CancellationChannel, CancellationConfig, CancellationSink, CancelledTask,
    LoggingCancellationSink, RecordingCancellationSink, SinkOutcome,
};
pub use durable_pool::{DurableOptions, DurableResourcePool};
pub use resource_pool::ResourcePool;
pub use shutdown::{ShutdownHandle, ShutdownPolicy, wait_for_signal};
pub use tokio_spawner::TokioSpawner;
#[cfg(not(target_arch = "wasm32"))]
pub use worker_pool::WorkerPool;
