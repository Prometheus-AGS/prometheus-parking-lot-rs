//! Builders to construct scheduler components from configuration.
//!
//! `build_pools_from_scheduler_config` builds every pool (in memory, or durable
//! on a configured backend) and opens every durable backend a
//! `config::SchedulerConfig` describes; `build_pools` builds in-memory pools
//! from queue, mailbox and executor factories you supply.

pub mod backends;
pub mod pool_builder;

#[cfg(feature = "embedded")]
pub use backends::RedbBackend;
#[cfg(feature = "postgres")]
pub use backends::{PgmqBackend, PostgresBackend};
#[cfg(any(feature = "postgres", feature = "embedded"))]
pub use pool_builder::ConfiguredDurablePool;
pub use pool_builder::{
    BuiltScheduler, ConfiguredPool, ConfiguredResourcePool, SharedInMemoryMailbox, build_pools,
    build_pools_from_scheduler_config, build_pools_from_scheduler_config_with_hooks,
};
