//! # Prometheus Parking Lot
//!
//! A configurable, runtime-agnostic parking-lot scheduler for AI agent workloads.
//!
//! This library provides a dedicated scheduling layer that manages resource-constrained
//! workloads across different deployment environments. It implements a "parking lot" pattern
//! where tasks are intelligently queued when system capacity is exhausted, then automatically
//! woken when resources become available.
//!
//! ## Core Problem Solved
//!
//! AI workloads have fundamentally different resource constraints than typical web services:
//!
//! - **GPU VRAM Limits**: Running multiple LLM inference tasks can exceed available GPU memory
//! - **Expensive Task Loss**: AI tasks are computationally expensive - losing work due to restarts is costly
//! - **Multi-Environment Deployment**: Same logic needs to work in desktop apps, cloud, and web
//! - **Disconnected Clients**: Long-running tasks may complete after clients disconnect
//!
//! ## Key Features
//!
//! - **Resource-Aware Scheduling**: Tracks resource consumption in arbitrary units
//! - **Parking Lot Algorithm**: Tasks queue when capacity exhausted, wake when available
//! - **Worker Thread Pool**: Dedicated OS threads for CPU/GPU-bound work (native) or async tasks (WASM)
//! - **Persistent Queues**: Survive application restarts
//! - **Mailbox System**: Store results for later retrieval when clients disconnect
//! - **Multi-Environment**: Same code runs on desktop (Tauri), cloud, and web (WASM)
//!
//! ## `WorkerPool` - Thread Pool for CPU/GPU-Bound Work
//!
//! The `WorkerPool` manages dedicated worker threads (on native) or async tasks (on WASM)
//! for CPU/GPU-bound work like LLM inference. This ensures heavy computation doesn't
//! block your main async runtime.
//!
//! ```rust,ignore
//! use prometheus_parking_lot::core::{WorkerPool, WorkerExecutor, TaskMetadata, SchedulerError};
//! use prometheus_parking_lot::config::WorkerPoolConfig;
//! use std::time::Duration;
//!
//! // Create a pool with 4 worker threads
//! let pool = WorkerPool::new(
//!     WorkerPoolConfig::new()
//!         .with_worker_count(4)
//!         .with_max_units(1000)
//!         .with_max_queue_depth(500),
//!     my_executor,  // Implements WorkerExecutor
//! )?;
//!
//! // Submit work (async API works on all platforms)
//! let key = pool.submit_async(job, meta).await?;
//! let result = pool.retrieve_async(&key, Duration::from_secs(120)).await?;
//!
//! // Blocking API available on native platforms only
//! #[cfg(not(target_arch = "wasm32"))]
//! {
//!     let key = pool.submit(job, meta)?;
//!     let result = pool.retrieve(&key, Duration::from_secs(120))?;
//! }
//! ```
//!
//! ## `ResourcePool` - Async Scheduling
//!
//! For lighter workloads that don't require dedicated threads, use `ResourcePool`:
//!
//! ```rust,ignore
//! use prometheus_parking_lot::core::{
//!     PoolLimits, ResourcePool, ScheduledTask, TaskExecutor, TaskMetadata,
//! };
//! use prometheus_parking_lot::infra::{queue::memory::InMemoryQueue, mailbox::memory::InMemoryMailbox};
//! use prometheus_parking_lot::util::serde::{Priority, ResourceCost, ResourceKind};
//! ```
//!
//! For complete examples, see:
//! - `crates/parking-lot-runtime-tokio/tests/parking_lot_algorithm_test.rs` - Full integration tests
//! - `README.md` - Comprehensive documentation

#![deny(missing_docs)]
#![deny(unsafe_code)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]

// Every Rust example in README.md runs as a doctest of this crate (cr-16).
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
mod readme_doctests {}

/// Builders to construct scheduler components from configuration.
#[cfg(feature = "tokio-runtime")]
pub mod builders;
/// Configuration models for pools, backends, and timeouts.
pub mod config;
/// Core scheduling abstractions and capacity accounting.
pub mod core;
/// Runtime adapters (native, web/worker, cloud) and API surface.
#[cfg(feature = "tokio-runtime")]
pub mod runtime;
/// Shared utilities.
pub mod util;

/// Infrastructure adapters for queues, mailboxes, and storage backends.
///
/// The in-memory backends come from `prometheus-parking-lot-core`; the
/// Postgres backends (feature `postgres`) from `prometheus-parking-lot-pg`; the
/// embedded file-backed backends (feature `embedded`) from
/// `prometheus-parking-lot-embedded`.
pub mod infra {
    /// Queue backends.
    pub mod queue {
        pub use prometheus_parking_lot_core::contracts::memory_queue as memory;
        #[cfg(feature = "embedded")]
        pub use prometheus_parking_lot_embedded::queue::yaque;
        #[cfg(feature = "postgres")]
        pub use prometheus_parking_lot_pg::queue as postgres;

        pub use memory::InMemoryQueue;
        #[cfg(feature = "postgres")]
        pub use postgres::PostgresQueue;
        pub use prometheus_parking_lot_core::contracts::memory_lease_queue::{
            DeadLetter, InMemoryLeaseQueue,
        };
        #[cfg(feature = "embedded")]
        pub use yaque::YaqueQueue;
    }

    /// Mailbox backends.
    pub mod mailbox {
        pub use prometheus_parking_lot_core::mailbox::memory;
        #[cfg(feature = "embedded")]
        pub use prometheus_parking_lot_embedded::mailbox::yaque;
        #[cfg(feature = "postgres")]
        pub use prometheus_parking_lot_pg::mailbox as postgres;

        pub use memory::InMemoryMailbox;
        #[cfg(feature = "postgres")]
        pub use postgres::PostgresMailbox;
        pub use prometheus_parking_lot_core::contracts::memory_result_mailbox::InMemoryResultMailbox;
        #[cfg(feature = "embedded")]
        pub use yaque::YaqueMailbox;
    }

    #[cfg(feature = "postgres")]
    pub use prometheus_parking_lot_pg::entities;

    pub use mailbox::InMemoryMailbox;
    #[cfg(feature = "embedded")]
    pub use mailbox::YaqueMailbox;
    pub use queue::InMemoryQueue;
    #[cfg(feature = "embedded")]
    pub use queue::YaqueQueue;
}

// ── Top-level convenience re-exports ─────────────────────────────────────────

#[cfg(any(
    all(feature = "tokio-runtime", not(target_arch = "wasm32")),
    target_arch = "wasm32"
))]
pub use crate::core::WorkerPool;
#[cfg(feature = "tokio-runtime")]
pub use crate::core::cancellation::{
    CancelReason, CancellationChannel, CancellationConfig, CancellationSink, CancelledTask,
    LoggingCancellationSink, RecordingCancellationSink, SinkOutcome,
};
pub use crate::core::clock::{Clock, SystemClock, TestClock};
pub use crate::core::error::{AppResult, SchedulerError, TaskError};
pub use crate::core::hooks::{
    CancelledReason, Hook, HookBus, LifecycleEvent, RecordingHook, RejectionReason, TracingHook,
};
#[cfg(feature = "tokio-runtime")]
pub use crate::core::shutdown::{ShutdownHandle, ShutdownPolicy, wait_for_signal};
pub use crate::core::{PoolStats, TaskMetadata};
