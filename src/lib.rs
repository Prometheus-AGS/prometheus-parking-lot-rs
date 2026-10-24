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
//! - **Multi-Environment**: Same code runs on desktop (Tauri), cloud, and web (WASM; on
//!   `wasm32-unknown-unknown` depend on this crate with `default-features = false`, because the
//!   default `tokio-runtime` feature does not build for that target)
//!
//! ## `WorkerPool` - Thread Pool for CPU/GPU-Bound Work
//!
//! The `WorkerPool` manages dedicated worker threads (on native) or async tasks (on WASM)
//! for CPU/GPU-bound work like LLM inference. This ensures heavy computation doesn't
//! block your main async runtime.
//!
//! ```rust
//! use std::time::Duration;
//!
//! use async_trait::async_trait;
//! use prometheus_parking_lot::WorkerPool;
//! use prometheus_parking_lot::config::WorkerPoolConfig;
//! use prometheus_parking_lot::core::{SchedulerError, TaskError, TaskMetadata, WorkerExecutor};
//! use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind, now_ms};
//!
//! /// Counts words; stands in for an inference call.
//! #[derive(Clone)]
//! struct WordCount;
//!
//! #[async_trait]
//! impl WorkerExecutor<String, usize> for WordCount {
//!     async fn execute(&self, text: String, _meta: TaskMetadata) -> Result<usize, TaskError> {
//!         Ok(text.split_whitespace().count())
//!     }
//! }
//!
//! fn meta(cpu_units: u32) -> TaskMetadata {
//!     TaskMetadata {
//!         id: 0,
//!         mailbox: None,
//!         priority: Priority::Normal,
//!         cost: ResourceCost::new(ResourceKind::Cpu, cpu_units),
//!         deadline_ms: None,
//!         created_at_ms: now_ms(),
//!         idempotency_key: None,
//!         sequence: 0, // the pool stamps the submission order
//!     }
//! }
//!
//! fn main() -> Result<(), SchedulerError> {
//!     // Four worker threads sharing 1000 units of capacity.
//!     let pool = WorkerPool::new(
//!         WorkerPoolConfig::new()
//!             .with_worker_count(4)
//!             .with_max_units(1000)
//!             .with_max_queue_depth(500),
//!         WordCount,
//!     )?;
//!
//!     // Blocking API (native only); from async code use `submit_async` and
//!     // `retrieve_async`, which take the same arguments.
//!     let key = pool.submit("the quick brown fox".to_owned(), meta(10))?;
//!     assert_eq!(pool.retrieve(&key, Duration::from_secs(120))?, 4);
//!     pool.shutdown();
//!     Ok(())
//! }
//! ```
//!
//! ## `ResourcePool` - Async Scheduling
//!
//! For lighter workloads that don't require dedicated threads, use `ResourcePool`.
//! It runs tasks on the async runtime and delivers each outcome to a mailbox
//! under the task's `MailboxKey`, where a client collects it later:
//!
//! ```rust
//! use std::time::Duration;
//!
//! use async_trait::async_trait;
//! use prometheus_parking_lot::builders::SharedInMemoryMailbox;
//! use prometheus_parking_lot::core::{
//!     PoolLimits, ResourcePool, ScheduledTask, SchedulerError, TaskError, TaskExecutor,
//!     TaskMetadata, TaskStatus,
//! };
//! use prometheus_parking_lot::infra::InMemoryQueue;
//! use prometheus_parking_lot::runtime::TokioSpawner;
//! use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};
//!
//! #[derive(Clone)]
//! struct Shout;
//!
//! #[async_trait]
//! impl TaskExecutor<String, String> for Shout {
//!     async fn execute(&self, text: String, _meta: TaskMetadata) -> Result<String, TaskError> {
//!         Ok(text.to_uppercase())
//!     }
//! }
//!
//! #[tokio::main]
//! async fn main() -> Result<(), SchedulerError> {
//!     let mailbox = SharedInMemoryMailbox::new();
//!     let pool = ResourcePool::new(
//!         PoolLimits {
//!             max_units: 4,
//!             max_queue_depth: 16,
//!             default_timeout: Duration::from_secs(30),
//!         },
//!         InMemoryQueue::new(16),
//!         mailbox.clone(),
//!         Shout,
//!         TokioSpawner::new(tokio::runtime::Handle::current()),
//!     );
//!
//!     let key = MailboxKey {
//!         tenant: "acme".into(),
//!         user_id: None,
//!         session_id: Some("session-1".into()),
//!     };
//!     let meta = TaskMetadata {
//!         id: 1,
//!         mailbox: Some(key.clone()),
//!         priority: Priority::Normal,
//!         cost: ResourceCost::new(ResourceKind::GpuVram, 2),
//!         deadline_ms: None,
//!         created_at_ms: now_ms(),
//!         idempotency_key: None,
//!         sequence: 0,
//!     };
//!     pool.submit(ScheduledTask { meta, payload: "hello".to_owned() }).await?;
//!
//!     // Collect the outcome from the mailbox, as a reconnecting client would.
//!     let mut outcome = None;
//!     for _ in 0..500 {
//!         outcome = mailbox.take(&key);
//!         if outcome.is_some() {
//!             break;
//!         }
//!         tokio::time::sleep(Duration::from_millis(10)).await;
//!     }
//!     assert_eq!(outcome, Some((TaskStatus::Completed, Some("HELLO".to_owned()))));
//!     Ok(())
//! }
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
/// embedded redb-backed backends (feature `embedded`) from
/// `prometheus-parking-lot-embedded`; Apache Iggy events and notifications
/// (feature `iggy`) from `prometheus-parking-lot-iggy`.
pub mod infra {
    /// Apache Iggy lifecycle events (`IggyHook`) and `mailbox.ready`
    /// notifications (`IggyNotifier`). Events and notification only: not a
    /// mailbox store and not a work queue.
    #[cfg(feature = "iggy")]
    pub mod events {
        pub use prometheus_parking_lot_iggy::*;
    }

    /// Queue backends.
    pub mod queue {
        pub use prometheus_parking_lot_core::contracts::memory_queue as memory;
        #[cfg(feature = "postgres")]
        pub use prometheus_parking_lot_pg::lease_queue as postgres;

        pub use memory::InMemoryQueue;
        #[cfg(feature = "postgres")]
        pub use postgres::PgLeaseQueue;
        pub use prometheus_parking_lot_core::contracts::memory_lease_queue::{
            DeadLetter, InMemoryLeaseQueue,
        };
        #[cfg(feature = "embedded")]
        pub use prometheus_parking_lot_embedded::RedbLeaseQueue;
        #[cfg(feature = "postgres")]
        pub use prometheus_parking_lot_pg::{PgmqLeaseQueue, QueueListener};
    }

    /// Mailbox backends.
    pub mod mailbox {
        pub use prometheus_parking_lot_core::mailbox::memory;
        #[cfg(feature = "postgres")]
        pub use prometheus_parking_lot_pg::mailbox as postgres;

        pub use memory::InMemoryMailbox;
        #[cfg(feature = "postgres")]
        pub use postgres::PgResultMailbox;
        pub use prometheus_parking_lot_core::contracts::memory_result_mailbox::InMemoryResultMailbox;
        #[cfg(feature = "embedded")]
        pub use prometheus_parking_lot_embedded::RedbResultMailbox;
    }

    pub use mailbox::InMemoryMailbox;
    #[cfg(feature = "embedded")]
    pub use mailbox::RedbResultMailbox;
    #[cfg(feature = "embedded")]
    pub use prometheus_parking_lot_embedded::EmbeddedStore;
    #[cfg(feature = "postgres")]
    pub use prometheus_parking_lot_pg::PgPool;
    pub use queue::InMemoryQueue;
    #[cfg(feature = "embedded")]
    pub use queue::RedbLeaseQueue;
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
pub use crate::core::shutdown::{ShutdownHandle, ShutdownPolicy, ShutdownReport, wait_for_signal};
pub use crate::core::{PoolStats, TaskMetadata};
/// The crash-durable resource pool (cr-49): accepted tasks persist in a
/// durable lease queue and results in a durable result mailbox. Built from
/// configuration by [`builders::build_pools_from_scheduler_config`] when a
/// pool names a backend, or started directly with
/// [`DurableResourcePool::start`].
#[cfg(feature = "tokio-runtime")]
pub use prometheus_parking_lot_runtime_tokio::durable_pool::{DurableOptions, DurableResourcePool};
