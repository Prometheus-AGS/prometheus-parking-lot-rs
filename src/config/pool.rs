//! Pool and scheduler configuration structures.
//!
//! The schema is strict: every struct rejects unknown fields, so a misspelled
//! field fails to load with an error that names it instead of being ignored.
//!
//! ```json
//! {
//!   "backends": {
//!     "local": { "type": "redb", "path": "./data/parking-lot.redb", "max_depth": 1000, "lease_secs": 300 }
//!   },
//!   "pools": {
//!     "inference": {
//!       "max_units": 8,
//!       "max_queue_depth": 200,
//!       "default_timeout_secs": 120,
//!       "queue": "in_memory",
//!       "mailbox": "in_memory"
//!     }
//!   }
//! }
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::backend::BackendConfig;
use super::error::ConfigError;

/// Where a pool keeps its parked tasks or its results.
///
/// Written as `"in_memory"` or as `{ "backend": "<name>" }`, naming a section
/// under `backends`.
///
/// Today only `"in_memory"` validates. A `ResourcePool` parks tasks and
/// delivers results through the synchronous `TaskQueue` and `Mailbox`
/// contracts; the durable backends implement the asynchronous lease
/// contracts, and pools start using them with durable dispatch (change
/// `cr-49`). Until then naming a backend here is rejected, so a configuration
/// never claims durability the pool does not provide. Use the
/// backend directly through the handle that
/// [`build_pools_from_scheduler_config`](crate::builders::build_pools_from_scheduler_config)
/// returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StorageConfig {
    /// In process memory; lost when the process exits.
    InMemory,
    /// The backend section with this name.
    Backend(String),
}

/// Pool configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(not(parking_lot_negative_control = "s-22"), serde(deny_unknown_fields))]
pub struct PoolConfig {
    /// Maximum resource units held by running tasks at once.
    pub max_units: u32,
    /// Maximum parked tasks; a submission that would exceed it is rejected
    /// with `QueueFull`.
    pub max_queue_depth: usize,
    /// Execution budget in seconds for a task without a nearer deadline: a
    /// task still running after it fails with `TaskError::Timeout`.
    pub default_timeout_secs: u64,
    /// Where parked tasks are kept.
    pub queue: StorageConfig,
    /// Where task outcomes are delivered.
    pub mailbox: StorageConfig,
}

/// Root scheduler configuration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(not(parking_lot_negative_control = "s-22"), serde(deny_unknown_fields))]
pub struct SchedulerConfig {
    /// Durable backends by name. Optional.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub backends: BTreeMap<String, BackendConfig>,
    /// Pools by name. At least one is required.
    pub pools: BTreeMap<String, PoolConfig>,
}

impl PoolConfig {
    /// Validate the pool's own values (limits and timeout).
    ///
    /// [`SchedulerConfig::validate`] also checks the `queue` and `mailbox`
    /// selections, which depend on the `backends` section.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] naming the first invalid field (zero units,
    /// queue depth or timeout).
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.validate_limits("")
    }

    fn validate_limits(&self, prefix: &str) -> Result<(), ConfigError> {
        if self.max_units == 0 {
            return Err(ConfigError::invalid(
                format!("{prefix}max_units"),
                "must be greater than 0",
            ));
        }
        if self.max_queue_depth == 0 {
            return Err(ConfigError::invalid(
                format!("{prefix}max_queue_depth"),
                "must be greater than 0",
            ));
        }
        if self.default_timeout_secs == 0 {
            return Err(ConfigError::invalid(
                format!("{prefix}default_timeout_secs"),
                "must be greater than 0",
            ));
        }
        Ok(())
    }
}

impl SchedulerConfig {
    /// Validate every backend and pool, and require at least one pool.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] naming the first invalid field by its dotted
    /// path, e.g. `pools.inference.max_units` or `backends.db.lease_secs`.
    /// Backends, then pools, are checked in name order.
    pub fn validate(&self) -> Result<(), ConfigError> {
        for (name, backend) in &self.backends {
            backend.validate(&format!("backends.{name}"))?;
        }
        if self.pools.is_empty() {
            return Err(ConfigError::invalid(
                "pools",
                "at least one pool must be defined",
            ));
        }
        for (name, pool) in &self.pools {
            let prefix = format!("pools.{name}.");
            pool.validate_limits(&prefix)?;
            self.validate_storage(&format!("{prefix}queue"), &pool.queue)?;
            self.validate_storage(&format!("{prefix}mailbox"), &pool.mailbox)?;
        }
        Ok(())
    }

    fn validate_storage(&self, field: &str, storage: &StorageConfig) -> Result<(), ConfigError> {
        match storage {
            StorageConfig::InMemory => Ok(()),
            StorageConfig::Backend(name) => {
                let Some(backend) = self.backends.get(name) else {
                    return Err(ConfigError::invalid(
                        field,
                        format!("no backend named `{name}` under `backends`"),
                    ));
                };
                Err(ConfigError::invalid(
                    field,
                    format!(
                        "pools cannot run on a durable backend yet (durable dispatch is not \
                         implemented); use \"in_memory\" here and use the {} backend `{name}` \
                         through the handle the builder returns",
                        backend.kind()
                    ),
                ))
            }
        }
    }

    /// Parse scheduler configuration from a JSON string and validate it.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Parse`] when the input is not valid JSON for this schema
    /// (the message names an unknown or missing field), or
    /// [`ConfigError::Invalid`] when a value fails [`validate`](Self::validate).
    pub fn from_json_str(input: &str) -> Result<Self, ConfigError> {
        let cfg: Self = serde_json::from_str(input)?;
        cfg.validate()?;
        Ok(cfg)
    }
}

pub use prometheus_parking_lot_core::config::WorkerPoolConfig;
