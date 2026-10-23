//! Opening the durable backends named in the `backends` configuration section.
//!
//! Each opened backend is returned as a handle holding the contract objects
//! the section configured. A pool configured on the backend runs on these
//! same objects (durable dispatch, change `cr-49`); applications can also use
//! them directly through [`LeaseQueue`](crate::core::LeaseQueue) and
//! [`ResultMailbox`](crate::core::ResultMailbox), for example to consume the
//! pool's results. A `pgmq` backend drives no pool; its queue is for the
//! application to use directly.

#[cfg(any(feature = "postgres", feature = "embedded"))]
use std::time::Duration;

#[cfg(feature = "postgres")]
use prometheus_parking_lot_pg::{
    PgLeaseQueue, PgPool, PgResultMailbox, PgmqLeaseQueue, QueueListener,
};

#[cfg(feature = "embedded")]
use prometheus_parking_lot_embedded::{EmbeddedStore, RedbLeaseQueue, RedbResultMailbox};

#[cfg(any(feature = "postgres", feature = "embedded"))]
use crate::core::SchedulerError;

/// An opened Postgres backend (a `"type": "postgres"` section): its lease
/// queue and result mailbox, sharing one connection pool.
#[cfg(feature = "postgres")]
#[derive(Debug)]
#[non_exhaustive]
pub struct PostgresBackend<P, T> {
    /// The lease queue named by `queue_name`, with `max_depth` and
    /// `max_attempts` applied.
    pub queue: PgLeaseQueue<P>,
    /// The result mailbox, also named by `queue_name`.
    pub mailbox: PgResultMailbox<T>,
    /// The wake-up listener, present when the section sets `listen: true`.
    pub listener: Option<QueueListener>,
    /// `lease_secs`: the duration to pass to
    /// [`LeaseQueue::lease`](crate::core::LeaseQueue::lease).
    pub lease_for: Duration,
}

/// An opened pgmq backend (a `"type": "pgmq"` section): a lease queue over
/// the pgmq extension. No pool runs on it; use the queue directly.
#[cfg(feature = "postgres")]
#[derive(Debug)]
#[non_exhaustive]
pub struct PgmqBackend<P> {
    /// The lease queue named by `queue_name`, with `max_depth` and
    /// `max_attempts` applied (and prepared when `prepare` is set).
    pub queue: PgmqLeaseQueue<P>,
    /// The enqueue listener, present when the section sets `listen: true`.
    pub listener: Option<QueueListener>,
    /// `lease_secs`: the duration to pass to
    /// [`LeaseQueue::lease`](crate::core::LeaseQueue::lease).
    pub lease_for: Duration,
}

/// An opened redb store (a `"type": "redb"` section): its lease queue and
/// result mailbox.
#[cfg(feature = "embedded")]
#[derive(Debug)]
#[non_exhaustive]
pub struct RedbBackend<P, T> {
    /// The open database; cloning it shares the file.
    pub store: EmbeddedStore,
    /// The lease queue, holding at most `max_depth` live entries.
    pub queue: RedbLeaseQueue<P>,
    /// The result mailbox in the same file.
    pub mailbox: RedbResultMailbox<T>,
    /// `lease_secs`: the duration to pass to
    /// [`LeaseQueue::lease`](crate::core::LeaseQueue::lease).
    pub lease_for: Duration,
}

/// Connect to Postgres, apply the queue and mailbox schemas when `migrate` is
/// set, and open the queue (and its listener when `listen` is set) and the
/// result mailbox on one connection pool.
#[cfg(feature = "postgres")]
pub(super) async fn open_postgres<P, T>(
    name: &str,
    cfg: &crate::config::PostgresBackendConfig,
) -> Result<PostgresBackend<P, T>, SchedulerError> {
    let pool = PgPool::connect(&cfg.url)
        .await
        .map_err(|e| SchedulerError::backend(format!("backends.{name}: connect"), e))?;
    if cfg.migrate {
        PgLeaseQueue::<P>::migrate(&pool)
            .await
            .map_err(|e| SchedulerError::backend(format!("backends.{name}: migrate queue"), e))?;
        PgResultMailbox::<T>::migrate(&pool)
            .await
            .map_err(|e| SchedulerError::backend(format!("backends.{name}: migrate mailbox"), e))?;
    }
    let mailbox = PgResultMailbox::new(pool.clone(), cfg.queue_name.clone());
    let mut queue = PgLeaseQueue::new(pool, cfg.queue_name.clone(), cfg.max_depth);
    if let Some(max_attempts) = cfg.max_attempts {
        queue = queue.with_max_attempts(max_attempts);
    }
    let listener = if cfg.listen {
        Some(
            queue
                .listener()
                .await
                .map_err(|e| SchedulerError::backend(format!("backends.{name}: listen"), e))?,
        )
    } else {
        None
    };
    Ok(PostgresBackend {
        queue,
        mailbox,
        listener,
        lease_for: Duration::from_secs(cfg.lease_secs),
    })
}

/// Connect to Postgres, open the pgmq queue, run its `prepare` when `prepare`
/// is set, and open its listener when `listen` is set.
#[cfg(feature = "postgres")]
pub(super) async fn open_pgmq<P>(
    name: &str,
    cfg: &crate::config::PgmqBackendConfig,
) -> Result<PgmqBackend<P>, SchedulerError> {
    let pool = PgPool::connect(&cfg.url)
        .await
        .map_err(|e| SchedulerError::backend(format!("backends.{name}: connect"), e))?;
    let mut queue = PgmqLeaseQueue::new(pool, cfg.queue_name.clone(), cfg.max_depth)?;
    if let Some(max_attempts) = cfg.max_attempts {
        queue = queue.with_max_attempts(max_attempts);
    }
    if cfg.prepare {
        queue
            .prepare()
            .await
            .map_err(|e| SchedulerError::backend(format!("backends.{name}: prepare"), e))?;
    }
    let listener = if cfg.listen {
        Some(
            queue
                .listener()
                .await
                .map_err(|e| SchedulerError::backend(format!("backends.{name}: listen"), e))?,
        )
    } else {
        None
    };
    Ok(PgmqBackend {
        queue,
        listener,
        lease_for: Duration::from_secs(cfg.lease_secs),
    })
}

/// Open (or create) the redb file and its queue and mailbox.
///
/// Opening is synchronous file IO; it runs once, at startup.
#[cfg(feature = "embedded")]
pub(super) fn open_redb<P, T>(
    name: &str,
    cfg: &crate::config::RedbBackendConfig,
) -> Result<RedbBackend<P, T>, SchedulerError>
where
    P: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
    T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static,
{
    let store = EmbeddedStore::open(&cfg.path)
        .map_err(|e| SchedulerError::backend(format!("backends.{name}: open"), e))?;
    let mut queue = store.lease_queue(cfg.max_depth);
    if let Some(max_attempts) = cfg.max_attempts {
        queue = queue.with_max_attempts(max_attempts);
    }
    Ok(RedbBackend {
        queue,
        mailbox: store.result_mailbox(),
        store,
        lease_for: Duration::from_secs(cfg.lease_secs),
    })
}
