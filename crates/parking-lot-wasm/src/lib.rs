//! # Prometheus Parking Lot - WASM
//!
//! The `wasm32` implementation of `WorkerPool`: async tasks with
//! semaphore-limited concurrency instead of OS threads, exposing the same API
//! as the native pool in `prometheus-parking-lot-runtime-tokio`.
//!
//! The implementation is compiled only for `target_arch = "wasm32"`; on every
//! other target this crate is empty, exactly as the module was before the
//! workspace split. Making it build and run on `wasm32-unknown-unknown` is
//! cr-38.

#[cfg(target_arch = "wasm32")]
mod pool;

#[cfg(target_arch = "wasm32")]
pub use pool::WorkerPool;
#[cfg(target_arch = "wasm32")]
pub use prometheus_parking_lot_core::{PoolStats, SchedulerError};
