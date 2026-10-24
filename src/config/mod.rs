//! Configuration models for pools, backends, and timeouts.
//!
//! `SchedulerConfig` is the root: named `pools` (limits plus where parked
//! tasks and results live) and optional named durable `backends` (Postgres,
//! redb, pgmq, and the iggy events backend) with every parameter needed to open them.
//! `builders::build_pools_from_scheduler_config` turns a validated
//! configuration into pools and opened backends. Example files:
//! `examples/configs/`.

pub mod backend;
pub mod error;
mod identity;
pub mod pool;

pub use backend::{
    BackendConfig, IggyBackendConfig, PgmqBackendConfig, PostgresBackendConfig, RedbBackendConfig,
};
pub use error::ConfigError;
pub use pool::{PoolConfig, SchedulerConfig, StorageConfig, WorkerPoolConfig};
