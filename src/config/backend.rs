//! Durable backend sections of the scheduler configuration.
//!
//! A backend section names a durable store and carries every parameter needed
//! to open it. [`build_pools_from_scheduler_config`] opens each one and hands
//! it back ready to use through the [`LeaseQueue`] and [`ResultMailbox`]
//! contracts.
//!
//! A pool whose `queue` and `mailbox` both name a backend runs on it durably
//! (change `cr-49`): the builder starts a `DurableResourcePool` that persists
//! each task in the backend's lease queue before accepting it, extends the
//! lease while the task waits or runs, and acks it only after the outcome is
//! stored in the backend's result mailbox.
//!
//! A `pgmq` backend opens a lease queue only. No pool runs on it: pool-level
//! crash durability on pgmq is not claimed (scenario S-18 proves lease
//! redelivery, fencing and priority order on pgmq, but the durable dispatch
//! loop is validated on the `postgres` backend), so validation rejects a pool
//! whose `queue` or `mailbox` names one. Applications use the opened queue
//! directly.
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
/// ```json
/// { "type": "pgmq", "url": "postgres://app@db/app", "queue_name": "events",
///   "max_depth": 10000, "lease_secs": 30, "max_attempts": 5, "listen": true, "prepare": true }
/// ```
///
/// Every section parses whatever features are enabled; validation rejects a
/// backend whose crate feature (`postgres` for `postgres` and `pgmq`,
/// `embedded` for `redb`) is off.
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
    /// A lease queue over the pgmq extension (`PgmqLeaseQueue`; feature
    /// `postgres`). A queue only: no pool can run on it (see the
    /// [module docs](self)).
    Pgmq(PgmqBackendConfig),
}

impl BackendConfig {
    /// The value of the `"type"` field.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Postgres(_) => "postgres",
            Self::Redb(_) => "redb",
            Self::Pgmq(_) => "pgmq",
        }
    }

    /// The lease duration that dispatchers pass to `LeaseQueue::lease`.
    #[must_use]
    pub const fn lease_for(&self) -> Duration {
        match self {
            Self::Postgres(cfg) => Duration::from_secs(cfg.lease_secs),
            Self::Redb(cfg) => Duration::from_secs(cfg.lease_secs),
            Self::Pgmq(cfg) => Duration::from_secs(cfg.lease_secs),
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
            Self::Pgmq(cfg) => cfg.validate(path),
        }
    }

    /// True when a pool can run on this backend (`postgres`, `redb`); false
    /// for `pgmq`, which provides a lease queue only.
    #[must_use]
    pub(crate) const fn can_drive_pool(&self) -> bool {
        match self {
            Self::Postgres(_) | Self::Redb(_) => true,
            Self::Pgmq(_) => false,
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
    /// other consumers before it is redelivered. A durable pool extends the
    /// lease every third of it while the task waits, runs or is delivered, so
    /// it bounds how soon work held by a crashed process is redelivered, not
    /// how long a task may run.
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

/// Parameters of a pgmq lease queue (`PgmqLeaseQueue`).
///
/// `Debug` hides the password in `url`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(not(parking_lot_negative_control = "s-22"), serde(deny_unknown_fields))]
pub struct PgmqBackendConfig {
    /// Connection URL of a database with the pgmq extension available (see
    /// [`PostgresBackendConfig::url`] on keeping passwords out of files).
    pub url: String,
    /// Logical queue name: 1 to 38 characters of `a-z`, `0-9` and `_`. It
    /// becomes part of the pgmq queue names (`pl_<name>_p0` .. `_p3`,
    /// `pl_<name>_dead`).
    pub queue_name: String,
    /// Maximum live (ready, delayed or leased) entries; `enqueue` beyond it
    /// fails with `QueueFull`.
    pub max_depth: usize,
    /// Lease duration in seconds: the pgmq visibility timeout `lease` sets.
    pub lease_secs: u64,
    /// Dead-letter a message once it has been leased this many times and is
    /// leased again. Absent: redeliver forever.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<u32>,
    /// Open a `LISTEN` connection for this queue's enqueue notifications
    /// (`QueueListener`). pgmq does not notify when a lease expires or a delay
    /// elapses, so consumers must still poll.
    #[serde(default)]
    pub listen: bool,
    /// Run `PgmqLeaseQueue::prepare` when the backend is opened: install the
    /// pgmq extension and side table if missing and create this queue's pgmq
    /// queues (idempotent). Leave `false` where they are created separately.
    #[serde(default)]
    pub prepare: bool,
}

/// Longest pgmq logical queue name: `pl_` + name + `_dead` must fit pgmq's
/// 47-character queue name (the rule `PgmqLeaseQueue::new` enforces).
const PGMQ_MAX_NAME_LEN: usize = 38;

impl PgmqBackendConfig {
    fn validate(&self, path: &str) -> Result<(), ConfigError> {
        if !cfg!(feature = "postgres") {
            return Err(ConfigError::invalid(
                format!("{path}.type"),
                "backend type `pgmq` requires the `postgres` feature of prometheus_parking_lot",
            ));
        }
        if self.url.trim().is_empty() {
            return Err(ConfigError::invalid(
                format!("{path}.url"),
                "must not be empty",
            ));
        }
        let name_ok = !self.queue_name.is_empty()
            && self.queue_name.len() <= PGMQ_MAX_NAME_LEN
            && self
                .queue_name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !name_ok {
            return Err(ConfigError::invalid(
                format!("{path}.queue_name"),
                format!("must be 1 to {PGMQ_MAX_NAME_LEN} characters of a-z, 0-9 and _"),
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

impl fmt::Debug for PgmqBackendConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PgmqBackendConfig")
            .field("url", &redact_password(&self.url))
            .field("queue_name", &self.queue_name)
            .field("max_depth", &self.max_depth)
            .field("lease_secs", &self.lease_secs)
            .field("max_attempts", &self.max_attempts)
            .field("listen", &self.listen)
            .field("prepare", &self.prepare)
            .finish()
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
