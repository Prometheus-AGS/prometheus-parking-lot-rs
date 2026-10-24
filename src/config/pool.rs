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
//!       "max_units": 32,
//!       "resource_limits": { "gpu_vram": 24, "cpu": 4 },
//!       "max_queue_depth": 200,
//!       "default_timeout_secs": 120,
//!       "queue": "in_memory",
//!       "mailbox": "in_memory"
//!     }
//!   }
//! }
//! ```

use std::collections::BTreeMap;

use prometheus_parking_lot_core::ResourceKind;
use serde::{Deserialize, Serialize};

use super::backend::BackendConfig;
use super::error::ConfigError;
use super::identity;

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
/// most one pool, and only a `postgres` or `redb` backend can drive one (a
/// `pgmq` backend is a queue for the application to use directly). Mixing a durable queue with an in-memory mailbox (or the
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
    /// Skip-ahead bound (cr-20): how many times a parked task that does not
    /// fit the free units may be passed by a smaller parked task that does,
    /// before the pool reserves capacity for it. `0` keeps strict head-of-line
    /// order. Optional; default 8. A pool on a durable backend dispatches
    /// strictly head-of-line and does not use it.
    #[serde(default = "default_max_head_skips")]
    pub max_head_skips: u32,
    /// Per-dimension limits (cr-23): units each resource kind may hold at
    /// once, e.g. `{"gpu_vram": 24, "cpu": 4}`. A task starts only when its
    /// cost fits `max_units` (the total over every kind) and every limit here
    /// at once; a kind not listed is bounded only by `max_units`. A task over
    /// any limit is rejected with `CapacityExceeded`. Optional; empty means
    /// single-dimension admission by `max_units` alone (the 0.2 behaviour).
    /// Each limit must be greater than zero and at most `max_units`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_limits: BTreeMap<ResourceKind, u32>,
}

/// The configuration key of `kind` (its serde name, e.g. `gpu_vram`).
fn kind_name(kind: ResourceKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{kind:?}"))
}

/// Default for [`PoolConfig::max_head_skips`]: the dispatcher's default.
const fn default_max_head_skips() -> u32 {
    prometheus_parking_lot_core::dispatch::DEFAULT_MAX_HEAD_SKIPS
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
        for (kind, &limit) in &self.resource_limits {
            let field = format!("{prefix}resource_limits.{}", kind_name(*kind));
            if limit == 0 {
                return Err(ConfigError::invalid(
                    field,
                    "must be greater than 0 (omit the kind to leave it bounded by max_units)",
                ));
            }
            if limit > self.max_units {
                return Err(ConfigError::invalid(
                    field,
                    format!(
                        "{limit} exceeds max_units ({}), so it could never be reached; raise \
                         max_units to at least the largest kind limit",
                        self.max_units
                    ),
                ));
            }
        }
        Ok(())
    }
}

impl SchedulerConfig {
    /// Validate every backend and pool, and require at least one pool.
    ///
    /// Names come first: a pool or backend name is 1 to 64 characters of ASCII
    /// letters, digits, `-` and `_` (the error path of a bad name is
    /// `pools["a b"]`). Then each backend, then that no two backends of one
    /// type open the same queue (the same redb `path`, or the same Postgres
    /// database and `queue_name`), then each pool. The queue comparison is
    /// lexical: it unifies `./a/../q.redb` with `q.redb` and
    /// `postgresql://U@HOST/db` with `postgres://V@host:5432/db`, but cannot
    /// see a symlink, a relative versus an absolute path, or a host name
    /// versus its IP address.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] naming the first invalid field by its dotted
    /// path, e.g. `pools.inference.max_units` or `backends.db.lease_secs`.
    /// Backends, then pools, are checked in name order.
    pub fn validate(&self) -> Result<(), ConfigError> {
        for name in self.backends.keys() {
            identity::validate_name("backends", name)?;
        }
        for name in self.pools.keys() {
            identity::validate_name("pools", name)?;
        }
        for (name, backend) in &self.backends {
            backend.validate(&format!("backends.{name}"))?;
        }
        self.validate_distinct_queues()?;
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

    /// Reject two backend sections of one type that open the same physical
    /// queue (the same redb file, or the same Postgres database and queue
    /// name): they would be two handles on one queue, which bypasses the rule
    /// that a durable backend drives at most one pool. Backends are compared in
    /// name order; the error is on the later section and names both.
    fn validate_distinct_queues(&self) -> Result<(), ConfigError> {
        let mut opened: BTreeMap<String, &str> = BTreeMap::new();
        for (name, backend) in &self.backends {
            let Some((key, field)) = identity::physical_queue(backend) else {
                continue;
            };
            if let Some(first) = opened.insert(key, name) {
                return Err(ConfigError::invalid(
                    format!("backends.{name}.{field}"),
                    format!(
                        "backends `{first}` and `{name}` are both `{}` sections that open the                          same queue; use one section (a second handle on it would bypass the                          one-pool-per-backend rule)",
                        backend.kind()
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Check a pool's `queue` and `mailbox`: both in memory, or both naming
    /// the same existing backend.
    fn validate_storage(&self, prefix: &str, pool: &PoolConfig) -> Result<(), ConfigError> {
        let exists = |field: &str, name: &str| match self.backends.get(name) {
            None => Err(ConfigError::invalid(
                format!("{prefix}{field}"),
                format!("no backend named `{name}` under `backends`"),
            )),
            Some(backend) if !backend.can_drive_pool() => Err(ConfigError::invalid(
                format!("{prefix}{field}"),
                format!(
                    "backend `{name}` is a `{}` queue, which no pool can run on: pool-level \
                     crash durability on it is not claimed; use a `postgres` or `redb` \
                     backend for a durable pool, or use the opened queue directly",
                    backend.kind()
                ),
            )),
            Some(_) => Ok(()),
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
