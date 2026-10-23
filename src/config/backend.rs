//! Durable backend sections of the scheduler configuration.
//!
//! A backend section names a durable store and carries every parameter needed
//! to open it. [`build_pools_from_scheduler_config`] opens each one and hands
//! it back ready to use through the [`LeaseQueue`] and [`ResultMailbox`]
//! contracts.
//!
//! Pools do not run on these backends yet: a `ResourcePool` parks tasks
//! through the synchronous `TaskQueue` contract, and the durable backends
//! implement the asynchronous lease contracts. Durable dispatch (change
//! `cr-49`) connects the two; until then a pool that names a backend for its
//! queue or mailbox is rejected at validation instead of quietly running
//! in memory.
//!
//! [`build_pools_from_scheduler_config`]: crate::builders::build_pools_from_scheduler_config
//! [`LeaseQueue`]: prometheus_parking_lot_core::LeaseQueue
//! [`ResultMailbox`]: prometheus_parking_lot_core::ResultMailbox

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::error::ConfigError;

/// One durable backend, selected by its `"type"` field.
///
/// ```json
/// { "type": "postgres", "url": "postgres://app@db/app", "queue_name": "inference",
///   "max_depth": 10000, "lease_secs": 300, "listen": true, "migrate": true }
/// ```
///
/// ```json
/// { "type": "redb", "path": "./data/parking-lot.redb", "max_depth": 1000, "lease_secs": 300 }
/// ```
///
/// Every section parses whatever features are enabled; validation rejects a
/// backend whose crate feature (`postgres`, `embedded`) is off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BackendConfig {
    /// The Postgres lease queue and result mailbox (`PgLeaseQueue`,
    /// `PgResultMailbox`; feature `postgres`).
    Postgres(PostgresBackendConfig),
    /// The embedded redb store: lease queue and result mailbox (feature
    /// `embedded`).
    Redb(RedbBackendConfig),
}

impl BackendConfig {
    /// The value of the `"type"` field.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Postgres(_) => "postgres",
            Self::Redb(_) => "redb",
        }
    }

    /// The lease duration that dispatchers pass to `LeaseQueue::lease`.
    #[must_use]
    pub const fn lease_for(&self) -> Duration {
        match self {
            Self::Postgres(cfg) => Duration::from_secs(cfg.lease_secs),
            Self::Redb(cfg) => Duration::from_secs(cfg.lease_secs),
        }
    }

    /// Validate the section; `path` is its dotted path (`backends.<name>`).
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] naming the first invalid field, including the
    /// `type` field when the backend's crate feature is not enabled.
    pub fn validate(&self, path: &str) -> Result<(), ConfigError> {
        match self {
            Self::Postgres(cfg) => cfg.validate(path),
            Self::Redb(cfg) => cfg.validate(path),
        }
    }
}

/// Parameters of a Postgres backend: a lease queue and a result mailbox.
///
/// `Debug` hides the password in `url`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(not(parking_lot_negative_control = "s-22"), serde(deny_unknown_fields))]
pub struct PostgresBackendConfig {
    /// Connection URL, e.g. `postgres://user:password@host:5432/database`.
    ///
    /// Configuration files are often committed; prefer injecting the URL from
    /// a secret store into the parsed configuration over writing a password
    /// into the file.
    pub url: String,
    /// Name of the queue and of the result mailbox. Queues (and mailboxes)
    /// with different names share the tables, not entries.
    pub queue_name: String,
    /// Maximum live (ready, delayed or leased) entries; `enqueue` beyond it
    /// fails with `QueueFull`.
    pub max_depth: usize,
    /// Lease duration in seconds: how long a leased task stays invisible to
    /// other consumers before it is redelivered. Must cover the longest task
    /// (or its heartbeat interval once leases are extended).
    pub lease_secs: u64,
    /// Dead-letter an entry after this many leases instead of redelivering it
    /// forever. Absent: redeliver forever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    /// Open a `LISTEN` connection for this queue's wake-up notifications
    /// (`QueueListener`), so consumers wake on enqueue instead of polling.
    #[serde(default)]
    pub listen: bool,
    /// Create the queue and mailbox tables, indexes and triggers when missing
    /// (idempotent).
    /// Leave `false` where migrations are applied separately.
    #[serde(default)]
    pub migrate: bool,
}

impl PostgresBackendConfig {
    fn validate(&self, path: &str) -> Result<(), ConfigError> {
        if !cfg!(feature = "postgres") {
            return Err(ConfigError::invalid(
                format!("{path}.type"),
                "backend type `postgres` requires the `postgres` feature of prometheus_parking_lot",
            ));
        }
        if self.url.trim().is_empty() {
            return Err(ConfigError::invalid(
                format!("{path}.url"),
                "must not be empty",
            ));
        }
        if self.queue_name.trim().is_empty() {
            return Err(ConfigError::invalid(
                format!("{path}.queue_name"),
                "must not be empty",
            ));
        }
        validate_common(path, self.max_depth, self.lease_secs)?;
        if self.max_attempts == Some(0) {
            return Err(ConfigError::invalid(
                format!("{path}.max_attempts"),
                "must be greater than 0 (omit it to redeliver forever)",
            ));
        }
        Ok(())
    }
}

impl fmt::Debug for PostgresBackendConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PostgresBackendConfig")
            .field("url", &redact_password(&self.url))
            .field("queue_name", &self.queue_name)
            .field("max_depth", &self.max_depth)
            .field("lease_secs", &self.lease_secs)
            .field("max_attempts", &self.max_attempts)
            .field("listen", &self.listen)
            .field("migrate", &self.migrate)
            .finish()
    }
}

/// Parameters of an embedded redb store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(not(parking_lot_negative_control = "s-22"), serde(deny_unknown_fields))]
pub struct RedbBackendConfig {
    /// Database file; created with its parent directories when missing. One
    /// process may hold a file open at a time.
    pub path: PathBuf,
    /// Maximum live entries in the lease queue.
    pub max_depth: usize,
    /// Lease duration in seconds (see [`PostgresBackendConfig::lease_secs`]).
    pub lease_secs: u64,
}

impl RedbBackendConfig {
    fn validate(&self, path: &str) -> Result<(), ConfigError> {
        if !cfg!(feature = "embedded") {
            return Err(ConfigError::invalid(
                format!("{path}.type"),
                "backend type `redb` requires the `embedded` feature of prometheus_parking_lot",
            ));
        }
        if self.path.as_os_str().is_empty() {
            return Err(ConfigError::invalid(
                format!("{path}.path"),
                "must not be empty",
            ));
        }
        validate_common(path, self.max_depth, self.lease_secs)
    }
}

fn validate_common(path: &str, max_depth: usize, lease_secs: u64) -> Result<(), ConfigError> {
    if max_depth == 0 {
        return Err(ConfigError::invalid(
            format!("{path}.max_depth"),
            "must be greater than 0",
        ));
    }
    if lease_secs == 0 {
        return Err(ConfigError::invalid(
            format!("{path}.lease_secs"),
            "must be greater than 0",
        ));
    }
    Ok(())
}

/// `scheme://user:password@host` → `scheme://user:***@host`.
fn redact_password(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let Some((userinfo, host)) = rest.rsplit_once('@') else {
        return url.to_owned();
    };
    match userinfo.split_once(':') {
        Some((user, _)) => format!("{scheme}://{user}:***@{host}"),
        None => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::redact_password;

    #[test]
    fn redacts_only_the_password() {
        assert_eq!(
            redact_password("postgres://app:s3cret@db:5432/app"),
            "postgres://app:***@db:5432/app"
        );
        assert_eq!(
            redact_password("postgres://app@db/app"),
            "postgres://app@db/app"
        );
        assert_eq!(redact_password("not a url"), "not a url");
    }
}
