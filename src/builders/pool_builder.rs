//! Builders to construct resource pools from configuration.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::config::{PoolConfig, SchedulerConfig};
use crate::core::{
    Hook, Mailbox, PoolLimits, ResourcePool, SchedulerError, Spawn, TaskExecutor, TaskPayload,
    TaskStatus,
};
use crate::infra::{InMemoryMailbox, InMemoryQueue};
use crate::util::MailboxKey;

#[cfg(feature = "embedded")]
use super::backends::RedbBackend;
#[cfg(feature = "postgres")]
use super::backends::{PgmqBackend, PostgresBackend};
#[cfg(any(feature = "postgres", feature = "embedded"))]
use crate::core::{MailboxEntry, ResultMailbox, ScheduledTask};
#[cfg(feature = "embedded")]
use prometheus_parking_lot_embedded::{RedbLeaseQueue, RedbResultMailbox};
#[cfg(feature = "postgres")]
use prometheus_parking_lot_pg::{PgLeaseQueue, PgResultMailbox};
#[cfg(any(feature = "postgres", feature = "embedded"))]
use prometheus_parking_lot_runtime_tokio::{DurableOptions, DurableResourcePool};

/// Build resource pools from scheduler configuration using provided factories.
///
/// Use this to supply your own queue and mailbox implementations;
/// [`build_pools_from_scheduler_config`] builds everything from the
/// configuration alone, including pools on durable backends.
///
/// # Errors
///
/// Returns [`SchedulerError::InvalidConfig`] when the configuration fails
/// [`SchedulerConfig::validate`] or a pool names a durable backend (those are
/// built by [`build_pools_from_scheduler_config`]), before any factory is
/// called; otherwise the error of the first queue, mailbox or executor factory
/// that fails.
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
    if let Some((name, _)) = cfg
        .pools
        .iter()
        .find(|(_, pool)| pool.durable_backend().is_some())
    {
        return Err(SchedulerError::invalid_config(format!(
            "pools.{name}.queue: build_pools builds pools on the queue and mailbox factories; \
             build a pool on a durable backend with build_pools_from_scheduler_config"
        )));
    }

    let mut pools = BTreeMap::new();
    for (name, pool_cfg) in &cfg.pools {
        let limits = limits_of(pool_cfg);

        let queue = queue_factory(name, pool_cfg)?;
        let mailbox = mailbox_factory(name, pool_cfg)?;
        let executor = executor_factory(name, pool_cfg)?;
        let pool = with_resource_limits(
            ResourcePool::<P, T, Q, M, E, S>::new(
                limits,
                queue,
                mailbox,
                executor,
                spawner.clone(),
            )
            .with_name(name.clone()),
            pool_cfg,
        )
        .with_max_head_skips(pool_cfg.max_head_skips);
        pools.insert(name.clone(), pool);
    }

    Ok(pools)
}

/// Apply a `pools` section's per-kind limits (cr-23) to a freshly built pool.
fn with_resource_limits<P, T, Q, M, E, S>(
    pool: ResourcePool<P, T, Q, M, E, S>,
    pool_cfg: &PoolConfig,
) -> ResourcePool<P, T, Q, M, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    pool_cfg
        .resource_limits
        .iter()
        .fold(pool, |pool, (&kind, &limit)| {
            pool.with_resource_limit(kind, limit)
        })
}

/// The pool limits a `pools` section configures.
const fn limits_of(pool: &PoolConfig) -> PoolLimits {
    PoolLimits {
        max_units: pool.max_units,
        max_queue_depth: pool.max_queue_depth,
        default_timeout: Duration::from_secs(pool.default_timeout_secs),
    }
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

/// A pool built from a `pools` section whose `queue` and `mailbox` name a
/// durable backend: a [`DurableResourcePool`] on that backend's lease queue and
/// result mailbox (durable dispatch, cr-49).
///
/// Submit through [`submit`](Self::submit) and collect outcomes with
/// [`consume`](Self::consume), or match the variant to reach the pool itself.
#[cfg(any(feature = "postgres", feature = "embedded"))]
#[non_exhaustive]
pub enum ConfiguredDurablePool<P, T, E, S> {
    /// A pool on a `"type": "postgres"` backend.
    #[cfg(feature = "postgres")]
    Postgres(DurableResourcePool<P, T, PgLeaseQueue<P>, PgResultMailbox<T>, E, S>),
    /// A pool on a `"type": "redb"` backend.
    #[cfg(feature = "embedded")]
    Redb(DurableResourcePool<P, T, RedbLeaseQueue<P>, RedbResultMailbox<T>, E, S>),
}

#[cfg(any(feature = "postgres", feature = "embedded"))]
impl<P, T, E, S> ConfiguredDurablePool<P, T, E, S>
where
    P: TaskPayload,
    T: Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    E: TaskExecutor<P, T>,
    S: Spawn + Send + Sync + 'static,
{
    /// Persist `task` in the backend's queue and accept it; see
    /// [`DurableResourcePool::submit`].
    ///
    /// # Errors
    ///
    /// As [`DurableResourcePool::submit`].
    pub async fn submit(&self, task: ScheduledTask<P>) -> Result<TaskStatus, SchedulerError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(pool) => pool.submit(task).await,
            #[cfg(feature = "embedded")]
            Self::Redb(pool) => pool.submit(task).await,
        }
    }

    /// Take the oldest outcome delivered under `key` from the backend's result
    /// mailbox (consume-once).
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the mailbox cannot be read.
    pub async fn consume(
        &self,
        key: &MailboxKey,
    ) -> Result<Option<MailboxEntry<T>>, SchedulerError> {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(pool) => pool.mailbox().consume(key).await,
            #[cfg(feature = "embedded")]
            Self::Redb(pool) => pool.mailbox().consume(key).await,
        }
    }

    /// Stop leasing; see [`DurableResourcePool::shutdown`].
    pub fn shutdown(&self) {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(pool) => pool.shutdown(),
            #[cfg(feature = "embedded")]
            Self::Redb(pool) => pool.shutdown(),
        }
    }

    /// Shut down under `policy`; see [`DurableResourcePool::shutdown_with`].
    pub async fn shutdown_with(
        &self,
        policy: crate::core::shutdown::ShutdownPolicy,
    ) -> crate::core::shutdown::ShutdownReport {
        match self {
            #[cfg(feature = "postgres")]
            Self::Postgres(pool) => pool.shutdown_with(policy).await,
            #[cfg(feature = "embedded")]
            Self::Redb(pool) => pool.shutdown_with(policy).await,
        }
    }
}

/// Everything a [`SchedulerConfig`] describes, built.
#[non_exhaustive]
pub struct BuiltScheduler<P, T, E, S>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    /// One entry per in-memory `pools` section, by name.
    pub pools: BTreeMap<String, ConfiguredPool<P, T, E, S>>,
    /// One entry per `pools` section on a durable backend, by name.
    #[cfg(any(feature = "postgres", feature = "embedded"))]
    pub durable_pools: BTreeMap<String, ConfiguredDurablePool<P, T, E, S>>,
    /// One entry per `"type": "postgres"` backend section, by name.
    #[cfg(feature = "postgres")]
    pub postgres: BTreeMap<String, PostgresBackend<P, T>>,
    /// One entry per `"type": "pgmq"` backend section, by name.
    #[cfg(feature = "postgres")]
    pub pgmq: BTreeMap<String, PgmqBackend<P>>,
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
/// the result mailbox; pgmq: connect, open the queue, prepare it when
/// `prepare` is set and open its listener when `listen` is set; redb: open or
/// create the file and its queue and mailbox), and each pool section is built, in name order, calling
/// `executor_factory` once per pool:
///
/// - a pool whose `queue` and `mailbox` are `"in_memory"` becomes a
///   `ResourcePool` with an in-memory queue and a [`SharedInMemoryMailbox`]
///   (in [`BuiltScheduler::pools`]);
/// - a pool whose `queue` and `mailbox` name a backend becomes a started
///   [`DurableResourcePool`] on that backend's lease queue and result mailbox,
///   leasing for the backend's `lease_secs` (in `BuiltScheduler::durable_pools`).
///   It begins leasing at once, so tasks a previous process accepted and did
///   not finish are redelivered to it as their leases expire.
///
/// # Errors
///
/// - [`SchedulerError::InvalidConfig`] when the configuration fails
///   [`SchedulerConfig::validate`]; the message names the field;
/// - [`SchedulerError::Backend`] when a backend cannot be opened (its context
///   names the section, e.g. `backends.db: connect`);
/// - the error of `executor_factory`.
///
/// To observe lifecycle events, use
/// [`build_pools_from_scheduler_config_with_hooks`].
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
    S: Spawn + Clone + Send + Sync + 'static,
{
    build_pools_from_scheduler_config_with_hooks(cfg, executor_factory, |_, _| Vec::new(), spawner)
        .await
}

/// [`build_pools_from_scheduler_config`], registering on each pool the
/// lifecycle hooks (cr-31) `hooks_factory` returns for it, by pool name.
///
/// The hooks are registered before the pool sees any task: an in-memory pool
/// gets them before it is returned, a durable pool through
/// `DurableOptions::with_hooks` before it starts leasing, so even a task
/// redelivered from a previous process is observed from its first event.
/// Hooks run on each pool's hook thread; return the same `Arc` for several
/// pools to observe them all with one hook.
///
/// # Errors
///
/// As [`build_pools_from_scheduler_config`].
pub async fn build_pools_from_scheduler_config_with_hooks<P, T, E, S, FE, FH>(
    cfg: &SchedulerConfig,
    mut executor_factory: FE,
    mut hooks_factory: FH,
    spawner: S,
) -> Result<BuiltScheduler<P, T, E, S>, SchedulerError>
where
    P: TaskPayload,
    T: Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    E: TaskExecutor<P, T>,
    FE: FnMut(&str, &PoolConfig) -> Result<E, SchedulerError>,
    FH: FnMut(&str, &PoolConfig) -> Vec<Arc<dyn Hook>>,
    S: Spawn + Clone + Send + Sync + 'static,
{
    cfg.validate()?;
    let opened = open_backends::<P, T>(cfg).await?;

    let mut pools = BTreeMap::new();
    #[cfg(any(feature = "postgres", feature = "embedded"))]
    let mut durable_pools = BTreeMap::new();
    for (name, pool_cfg) in &cfg.pools {
        let executor = executor_factory(name, pool_cfg)?;
        let hooks = hooks_factory(name, pool_cfg);
        let limits = limits_of(pool_cfg);
        let Some(backend) = pool_cfg.durable_backend() else {
            let mailbox = SharedInMemoryMailbox::new();
            let pool = with_resource_limits(
                ResourcePool::new(
                    limits,
                    InMemoryQueue::new(pool_cfg.max_queue_depth),
                    mailbox.clone(),
                    executor,
                    spawner.clone(),
                )
                .with_name(name.clone()),
                pool_cfg,
            )
            .with_max_head_skips(pool_cfg.max_head_skips);
            for hook in hooks {
                pool.hooks().register(hook);
            }
            pools.insert(name.clone(), ConfiguredPool { pool, mailbox });
            continue;
        };
        #[cfg(any(feature = "postgres", feature = "embedded"))]
        {
            let durable = opened.start_durable_pool(
                cfg,
                name,
                backend,
                limits,
                executor,
                spawner.clone(),
                hooks,
            )?;
            durable_pools.insert(name.clone(), durable);
        }
        #[cfg(not(any(feature = "postgres", feature = "embedded")))]
        {
            // `validate` rejects every backend section without its feature,
            // so no pool can name one here.
            let _ = (executor, limits, hooks);
            return Err(SchedulerError::invalid_config(format!(
                "pools.{name}.queue: backend `{backend}` is not enabled"
            )));
        }
    }

    let Opened {
        #[cfg(feature = "postgres")]
        postgres,
        #[cfg(feature = "postgres")]
        pgmq,
        #[cfg(feature = "embedded")]
        redb,
        ..
    } = opened;
    Ok(BuiltScheduler {
        pools,
        #[cfg(any(feature = "postgres", feature = "embedded"))]
        durable_pools,
        #[cfg(feature = "postgres")]
        postgres,
        #[cfg(feature = "postgres")]
        pgmq,
        #[cfg(feature = "embedded")]
        redb,
    })
}

/// The backends opened from a configuration's `backends` section.
struct Opened<P, T> {
    #[cfg(feature = "postgres")]
    postgres: BTreeMap<String, PostgresBackend<P, T>>,
    #[cfg(feature = "postgres")]
    pgmq: BTreeMap<String, PgmqBackend<P>>,
    #[cfg(feature = "embedded")]
    redb: BTreeMap<String, RedbBackend<P, T>>,
    _types: std::marker::PhantomData<fn() -> (P, T)>,
}

/// Open every backend section, in name order.
#[cfg_attr(
    not(feature = "postgres"),
    allow(
        clippy::unused_async,
        reason = "only opening a Postgres backend awaits; the signature is the same for every feature set"
    )
)]
#[cfg_attr(
    not(any(feature = "postgres", feature = "embedded")),
    allow(
        unused_mut,
        clippy::never_loop,
        reason = "without a backend feature there is no map to fill and every section is rejected"
    )
)]
async fn open_backends<P, T>(cfg: &SchedulerConfig) -> Result<Opened<P, T>, SchedulerError>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    let mut opened = Opened {
        #[cfg(feature = "postgres")]
        postgres: BTreeMap::new(),
        #[cfg(feature = "postgres")]
        pgmq: BTreeMap::new(),
        #[cfg(feature = "embedded")]
        redb: BTreeMap::new(),
        _types: std::marker::PhantomData,
    };
    for (name, backend) in &cfg.backends {
        match backend {
            #[cfg(feature = "postgres")]
            crate::config::BackendConfig::Postgres(section) => {
                opened.postgres.insert(
                    name.clone(),
                    super::backends::open_postgres(name, section).await?,
                );
            }
            #[cfg(feature = "postgres")]
            crate::config::BackendConfig::Pgmq(section) => {
                opened.pgmq.insert(
                    name.clone(),
                    super::backends::open_pgmq(name, section).await?,
                );
            }
            #[cfg(feature = "embedded")]
            crate::config::BackendConfig::Redb(section) => {
                opened
                    .redb
                    .insert(name.clone(), super::backends::open_redb(name, section)?);
            }
            // An events backend opens nothing here: the application builds its
            // hook and notifier from `IggyBackendConfig::options`, because they
            // are handed to pools and mailboxes the builder does not own.
            crate::config::BackendConfig::Iggy(_) => {}
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
    Ok(opened)
}

#[cfg(any(feature = "postgres", feature = "embedded"))]
impl<P, T> Opened<P, T>
where
    P: TaskPayload,
    T: Clone + Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
{
    /// Start the durable pool `name` on the opened backend `backend`, leasing
    /// for the backend's `lease_secs` and emitting lifecycle events to `hooks`.
    #[expect(
        clippy::too_many_arguments,
        reason = "private helper; each argument is one piece of the pool"
    )]
    fn start_durable_pool<E, S>(
        &self,
        cfg: &SchedulerConfig,
        name: &str,
        backend: &str,
        limits: PoolLimits,
        executor: E,
        spawner: S,
        hooks: Vec<Arc<dyn Hook>>,
    ) -> Result<ConfiguredDurablePool<P, T, E, S>, SchedulerError>
    where
        E: TaskExecutor<P, T>,
        S: Spawn + Send + Sync + 'static,
    {
        let section = &cfg.backends[backend];
        let bus = crate::core::HookBus::new();
        for hook in hooks {
            bus.register(hook);
        }
        let options = cfg.pools[name].resource_limits.iter().fold(
            DurableOptions::new(section.lease_for())
                .with_name(name)
                .with_hooks(bus),
            |options, (&kind, &limit)| options.with_resource_limit(kind, limit),
        );
        match section {
            #[cfg(feature = "postgres")]
            crate::config::BackendConfig::Postgres(_) => {
                let opened = &self.postgres[backend];
                Ok(ConfiguredDurablePool::Postgres(DurableResourcePool::start(
                    limits,
                    opened.queue.clone(),
                    opened.mailbox.clone(),
                    executor,
                    spawner,
                    options,
                )?))
            }
            #[cfg(feature = "embedded")]
            crate::config::BackendConfig::Redb(redb) => {
                let opened = &self.redb[backend];
                Ok(ConfiguredDurablePool::Redb(DurableResourcePool::start(
                    limits,
                    opened.store.lease_queue(redb.max_depth),
                    opened.store.result_mailbox(),
                    executor,
                    spawner,
                    options,
                )?))
            }
            // `validate` rejects a pool on a backend that cannot drive one.
            other if !other.can_drive_pool() => Err(SchedulerError::invalid_config(format!(
                "pools.{name}.queue: backend `{backend}` of type `{}` cannot drive a pool",
                other.kind()
            ))),
            #[allow(
                unreachable_patterns,
                reason = "reachable only when a backend's feature is off"
            )]
            other => Err(SchedulerError::invalid_config(format!(
                "backends.{backend}.type: backend type `{}` is not enabled",
                other.kind()
            ))),
        }
    }
}
