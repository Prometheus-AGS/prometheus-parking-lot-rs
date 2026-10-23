//! Builders to construct scheduler components from configuration.
//!
//! `build_pools_from_scheduler_config` builds every pool and opens every
//! durable backend a `config::SchedulerConfig` describes; `build_pools` builds
//! pools from queue, mailbox and executor factories you supply.

pub mod backends;
pub mod pool_builder;

#[cfg(feature = "postgres")]
pub use backends::PostgresBackend;
#[cfg(feature = "embedded")]
pub use backends::RedbBackend;
pub use pool_builder::{
    BuiltScheduler, ConfiguredPool, ConfiguredResourcePool, SharedInMemoryMailbox, build_pools,
    build_pools_from_scheduler_config,
};
