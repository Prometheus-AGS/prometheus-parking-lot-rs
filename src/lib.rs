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
//! - **Persistent Queues**: Survive application restarts
//! - **Mailbox System**: Store results for later retrieval when clients disconnect
//! - **Multi-Environment**: Same code runs on desktop (Tauri), cloud, and web
//!
//! ## Quick Example
//!
//! ```rust,no_run
//! use prometheus_parking_lot::core::{
//!     PoolLimits, ResourcePool, ScheduledTask, TaskExecutor, TaskMetadata,
//! };
//! use prometheus_parking_lot::infra::{queue::memory::InMemoryQueue, mailbox::memory::InMemoryMailbox};
//! use prometheus_parking_lot::util::serde::{Priority, ResourceCost, ResourceKind};
//! use prometheus_parking_lot::util::clock::now_ms;
//! use std::time::Duration;
//!
//! // See tests/parking_lot_algorithm_test.rs for complete working examples
//! ```
//!
//! For complete examples, see:
//! - `tests/parking_lot_algorithm_test.rs` - Full integration tests
//! - `README.md` - Comprehensive documentation

#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_code)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![warn(clippy::nursery)]

/// Core scheduling abstractions and capacity accounting.
pub mod core;
/// Configuration models for pools, backends, and timeouts.
pub mod config;
/// Builders to construct scheduler components from configuration.
pub mod builders;
/// Infrastructure adapters for queues, mailboxes, and storage backends.
pub mod infra;
/// Runtime adapters (native, web/worker, cloud) and API surface.
pub mod runtime;
/// Shared utilities.
pub mod util;

