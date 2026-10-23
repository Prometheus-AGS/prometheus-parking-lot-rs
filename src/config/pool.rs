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
/// A pool is either entirely in memory (`queue` and `mailbox` both
/// `"in_memory"`) or durable: `queue` and `mailbox` both name the same
/// backend. A durable pool persists every accepted task in the backend's lease
/// queue before `submit` returns and delivers every outcome to the backend's
/// result mailbox before it acks the task, so accepted work survives a crash
/// of the process (durable dispatch, change `cr-49`). Each backend drives at
/// most one pool. Mixing a durable queue with an in-memory mailbox (or the
/// reverse) is rejected: either half in memory would lose work or results at
/// a crash.
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
    /// with `QueueFull`. For a pool on a durable backend the backend's
    /// `max_depth` bounds accepted work instead, and this value is only
    /// validated.
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
    /// The durable backend this pool runs on, when its `queue` names one.
    /// Validation guarantees the `mailbox` names the same backend.
    pub(crate) fn durable_backend(&self) -> Option<&str> {
        match &self.queue {
            StorageConfig::Backend(name) => Some(name),
            StorageConfig::InMemory => None,
        }
    }

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
        let mut driven_by: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, pool) in &self.pools {
            let prefix = format!("pools.{name}.");
            pool.validate_limits(&prefix)?;
            self.validate_storage(&prefix, pool)?;
            if let Some(backend) = pool.durable_backend()
                && let Some(other) = driven_by.insert(backend, name)
            {
                return Err(ConfigError::invalid(
                    format!("{prefix}queue"),
                    format!(
                        "backend `{backend}` already drives pool `{other}`; each durable \
                         backend drives at most one pool"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Check a pool's `queue` and `mailbox`: both in memory, or both naming
    /// the same existing backend.
    fn validate_storage(&self, prefix: &str, pool: &PoolConfig) -> Result<(), ConfigError> {
        let exists = |field: &str, name: &str| {
            if self.backends.contains_key(name) {
                Ok(())
            } else {
                Err(ConfigError::invalid(
                    format!("{prefix}{field}"),
                    format!("no backend named `{name}` under `backends`"),
                ))
            }
        };
        match (&pool.queue, &pool.mailbox) {
            (StorageConfig::InMemory, StorageConfig::InMemory) => Ok(()),
            (StorageConfig::Backend(queue), StorageConfig::Backend(mailbox)) => {
                exists("queue", queue)?;
                exists("mailbox", mailbox)?;
                if queue == mailbox {
                    Ok(())
                } else {
                    Err(ConfigError::invalid(
                        format!("{prefix}mailbox"),
                        format!(
                            "must name the same backend as `queue` (`{queue}`): a durable pool \
                             acks a task only after its outcome is stored in that backend"
                        ),
                    ))
                }
            }
            (StorageConfig::Backend(queue), StorageConfig::InMemory) => {
                exists("queue", queue)?;
                Err(ConfigError::invalid(
                    format!("{prefix}mailbox"),
                    format!(
                        "a pool on durable backend `{queue}` must deliver results there too \
                         (set {{\"backend\": \"{queue}\"}}); an in-memory mailbox loses \
                         results at a crash"
                    ),
                ))
            }
            (StorageConfig::InMemory, StorageConfig::Backend(mailbox)) => {
                exists("mailbox", mailbox)?;
                Err(ConfigError::invalid(
                    format!("{prefix}queue"),
                    format!(
                        "a pool delivering to durable backend `{mailbox}` must queue there too \
                         (set {{\"backend\": \"{mailbox}\"}}); an in-memory queue loses \
                         accepted tasks at a crash"
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
