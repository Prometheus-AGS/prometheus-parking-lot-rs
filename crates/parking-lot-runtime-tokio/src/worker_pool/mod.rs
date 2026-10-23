//! Native `WorkerPool`: dedicated OS worker threads, each with its own
//! single-threaded tokio runtime, for CPU/GPU-bound work.
//!
//! # Key Features
//!
//! - **Dual API**: Blocking (`submit`/`retrieve`) and async (`submit_async`/`retrieve_async`)
//! - **No serialization requirement**: Result types can include channels, handles, etc.
//! - **Resource-aware**: Capacity-based admission control with queue overflow
//!
//! The WASM implementation of the same API lives in `prometheus-parking-lot-wasm`;
//! the shared error and statistics types live in the core crate.
//!
//! # Example
//!
//! ```rust,ignore
//! use prometheus_parking_lot::core::{WorkerPool, WorkerExecutor, TaskMetadata};
//! use prometheus_parking_lot::config::WorkerPoolConfig;
//!
//! // Create a pool
//! let pool = WorkerPool::new(
//!     WorkerPoolConfig::new()
//!         .with_worker_count(4)
//!         .with_max_units(1000),
//!     my_executor,
//! )?;
//!
//! // Submit work (async API works everywhere)
//! let key = pool.submit_async(job, meta).await?;
//! let result = pool.retrieve_async(&key, Duration::from_secs(60)).await?;
//! ```

mod native;
mod slots;

pub use native::WorkerPool;
pub use prometheus_parking_lot_core::PoolStats;
