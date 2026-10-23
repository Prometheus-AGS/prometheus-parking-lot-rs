//! Builders to construct resource pools from configuration.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::config::{PoolConfig, SchedulerConfig};
use crate::core::{
    Mailbox, PoolLimits, ResourcePool, SchedulerError, TaskExecutor, TaskPayload, TaskStatus,
};
use crate::infra::{InMemoryMailbox, InMemoryQueue};
use crate::util::MailboxKey;

#[cfg(feature = "postgres")]
use super::backends::PostgresBackend;
#[cfg(feature = "embedded")]
use super::backends::RedbBackend;

/// Build resource pools from scheduler configuration using provided factories.
///
/// Use this to supply your own queue and mailbox implementations;
/// [`build_pools_from_scheduler_config`] builds everything from the
/// configuration alone.
///
/// # Errors
///
/// Returns [`SchedulerError::InvalidConfig`] when the configuration fails
/// [`SchedulerConfig::validate`] (before any factory is called), or the error
/// of the first queue, mailbox or executor factory that fails.
#[expect(
    clippy::type_complexity,
    reason = "the generic pool map is part of the public builder signature"
)]
pub fn build_pools<P, T, Q, M, E, S, FQ, FM, FE>(
    cfg: &SchedulerConfig,
    mut queue_factory: FQ,
    mut mailbox_factory: FM,
    mut executor_factory: FE,
    spawner: S,
) -> Result<BTreeMap<String, ResourcePool<P, T, Q, M, E, S>>, SchedulerError>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    E: TaskExecutor<P, T>,
    FQ: FnMut(&str, &PoolConfig) -> Result<Q, SchedulerError>,
    FM: FnMut(&str, &PoolConfig) -> Result<M, SchedulerError>,
    FE: FnMut(&str, &PoolConfig) -> Result<E, SchedulerError>,
    S: Clone,
{
    cfg.validate()?;

    let mut pools = BTreeMap::new();
    for (name, pool_cfg) in &cfg.pools {
        let limits = PoolLimits {
            max_units: pool_cfg.max_units,
            max_queue_depth: pool_cfg.max_queue_depth,
            default_timeout: Duration::from_secs(pool_cfg.default_timeout_secs),
        };

        let queue = queue_factory(name, pool_cfg)?;
        let mailbox = mailbox_factory(name, pool_cfg)?;
        let executor = executor_factory(name, pool_cfg)?;
        let pool = ResourcePool::<P, T, Q, M, E, S>::new(
            limits,
            queue,
            mailbox,
            executor,
            spawner.clone(),
        );
        pools.insert(name.clone(), pool);
    }

    Ok(pools)
}

/// An in-memory mailbox shared between a pool, which delivers outcomes into
/// it, and the application, which collects them.
///
/// A `ResourcePool` owns its mailbox, so the builder gives each pool a clone of
/// one of these and returns another clone in [`ConfiguredPool::mailbox`].
pub struct SharedInMemoryMailbox<T> {
    inner: Arc<Mutex<InMemoryMailbox<T>>>,
}

impl<T> Clone for SharedInMemoryMailbox<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T> Default for SharedInMemoryMailbox<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> std::fmt::Debug for SharedInMemoryMailbox<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedInMemoryMailbox")
            .field("len", &self.lock().len())
            .finish()
    }
}

impl<T> SharedInMemoryMailbox<T> {
    /// An empty mailbox.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(InMemoryMailbox::new())),
        }
    }

    /// Number of outcomes waiting to be collected.
    #[must_use]
    pub fn len(&self) -> usize {
        self.lock().len()
    }

    /// True when no outcome is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    /// Take the outcome delivered under `key`, if any (consume-once).
    #[must_use]
    pub fn take(&self, key: &MailboxKey) -> Option<(TaskStatus, Option<T>)>
    where
        T: Clone,
    {
        // `InMemoryMailbox::consume` cannot fail.
        self.lock().consume(key).ok().flatten()
    }

    /// The lock never guards a half-finished update (every `InMemoryMailbox`
    /// method completes or does nothing), so a poisoned lock is still usable.
    fn lock(&self) -> MutexGuard<'_, InMemoryMailbox<T>> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<T: Clone> Mailbox<T> for SharedInMemoryMailbox<T> {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<T>,
    ) -> Result<(), SchedulerError> {
        self.lock().deliver(key, status, payload)
    }

    fn deliver_with_ttl(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<T>,
        ttl_ms: u64,
    ) -> Result<(), SchedulerError> {
        self.lock().deliver_with_ttl(key, status, payload, ttl_ms)
    }

    fn consume(
        &mut self,
        key: &MailboxKey,
    ) -> Result<Option<(TaskStatus, Option<T>)>, SchedulerError> {
        self.lock().consume(key)
    }

    fn evict_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        self.lock().evict_expired(now_ms)
    }
}

/// The `ResourcePool` type a configuration builds: in-memory queue, shared
/// in-memory mailbox.
pub type ConfiguredResourcePool<P, T, E, S> =
    ResourcePool<P, T, InMemoryQueue<P>, SharedInMemoryMailbox<T>, E, S>;

/// One pool built from a `pools` section.
#[non_exhaustive]
pub struct ConfiguredPool<P, T, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    /// The pool.
    pub pool: ConfiguredResourcePool<P, T, E, S>,
    /// The pool's mailbox: collect task outcomes here by `MailboxKey`.
    pub mailbox: SharedInMemoryMailbox<T>,
}

/// Everything a [`SchedulerConfig`] describes, built.
#[non_exhaustive]
pub struct BuiltScheduler<P, T, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    /// One entry per `pools` section, by name.
    pub pools: BTreeMap<String, ConfiguredPool<P, T, E, S>>,
    /// One entry per `"type": "postgres"` backend section, by name.
    #[cfg(feature = "postgres")]
    pub postgres: BTreeMap<String, PostgresBackend<P, T>>,
    /// One entry per `"type": "redb"` backend section, by name.
    #[cfg(feature = "embedded")]
    pub redb: BTreeMap<String, RedbBackend<P, T>>,
}

/// Build every pool and open every backend a scheduler configuration
/// describes.
///
/// The configuration is validated first; nothing is opened for an invalid
/// one. Then each backend section is opened (Postgres: connect, migrate when
/// `migrate` is set, open the queue, its listener when `listen` is set, and
/// the result mailbox; redb: open or create the file and its queue and
/// mailbox), and each pool section becomes a `ResourcePool` with an in-memory queue and a
/// [`SharedInMemoryMailbox`]. `executor_factory` is called once per pool, in
/// name order.
///
/// Pools do not run on the durable backends yet (durable dispatch is change
/// `cr-49`); validation rejects a pool whose `queue` or `mailbox` names one.
/// Use the returned backend handles directly.
///
/// # Errors
///
/// - [`SchedulerError::InvalidConfig`] when the configuration fails
///   [`SchedulerConfig::validate`]; the message names the field;
/// - [`SchedulerError::Backend`] when a backend cannot be opened (its context
///   names the section, e.g. `backends.db: connect`);
/// - the error of `executor_factory`.
#[cfg_attr(
    not(feature = "postgres"),
    allow(
        clippy::unused_async,
        reason = "only opening a Postgres backend awaits; the signature is the same for every feature set"
    )
)]
pub async fn build_pools_from_scheduler_config<P, T, E, S, FE>(
    cfg: &SchedulerConfig,
    executor_factory: FE,
    spawner: S,
) -> Result<BuiltScheduler<P, T, E, S>, SchedulerError>
where
    P: TaskPayload,
    T: Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    E: TaskExecutor<P, T>,
    FE: FnMut(&str, &PoolConfig) -> Result<E, SchedulerError>,
    S: Clone,
{
    cfg.validate()?;

    #[cfg(feature = "postgres")]
    let mut postgres = BTreeMap::new();
    #[cfg(feature = "embedded")]
    let mut redb = BTreeMap::new();
    for (name, backend) in &cfg.backends {
        match backend {
            #[cfg(feature = "postgres")]
            crate::config::BackendConfig::Postgres(section) => {
                postgres.insert(
                    name.clone(),
                    super::backends::open_postgres(name, section).await?,
                );
            }
            #[cfg(feature = "embedded")]
            crate::config::BackendConfig::Redb(section) => {
                redb.insert(name.clone(), super::backends::open_redb(name, section)?);
            }
            #[allow(
                unreachable_patterns,
                reason = "reachable only when a backend's feature is off"
            )]
            other => {
                // `validate` rejects a backend whose feature is off.
                return Err(SchedulerError::invalid_config(format!(
                    "backends.{name}.type: backend type `{}` is not enabled",
                    other.kind()
                )));
            }
        }
    }

    let mut mailboxes = BTreeMap::new();
    let pools = build_pools::<P, T, _, _, E, S, _, _, _>(
        cfg,
        |_, pool| Ok(InMemoryQueue::new(pool.max_queue_depth)),
        |name, _| {
            let mailbox = SharedInMemoryMailbox::new();
            mailboxes.insert(name.to_owned(), mailbox.clone());
            Ok(mailbox)
        },
        executor_factory,
        spawner,
    )?;
    let pools = pools
        .into_iter()
        .map(|(name, pool)| {
            let mailbox = mailboxes
                .remove(&name)
                .unwrap_or_else(|| unreachable!("build_pools made a mailbox for pool `{name}`"));
            (name, ConfiguredPool { pool, mailbox })
        })
        .collect();

    Ok(BuiltScheduler {
        pools,
        #[cfg(feature = "postgres")]
        postgres,
        #[cfg(feature = "embedded")]
        redb,
    })
}
