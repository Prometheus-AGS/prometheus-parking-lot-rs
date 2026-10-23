//! The pools under test (cr-40), behind one submit-and-observe interface:
//! the in-memory `ResourcePool`, the native `WorkerPool`, and the
//! `DurableResourcePool` on the in-memory reference backends and on redb.
//!
//! Every pool runs the same executor ([`Work`]): it stamps the start, holds
//! its units for a fixed service time (zero for a pure dispatch measurement),
//! stamps the end and returns the task id. The result is "readable" when:
//!
//! - `ResourcePool`: the outcome is stored in its mailbox (a wrapper around
//!   `InMemoryMailbox` stamps it after the store);
//! - `WorkerPool`: `retrieve_async` returned it to a waiter task spawned at
//!   submission (one per task, as a concurrent client would do);
//! - `DurableResourcePool`: the result mailbox's `deliver` resolved (a wrapper
//!   stamps it after the inner store committed; the lease ack follows).

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
#[cfg(feature = "embedded")]
use tempfile::TempDir;

use prometheus_parking_lot::config::WorkerPoolConfig;
use prometheus_parking_lot::core::{
    Delivery, DeliveryOutcome, Mailbox, MailboxEntry, PoolLimits, ResourcePool, ResultMailbox,
    ScheduledTask, SchedulerError, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
    WorkerExecutor, WorkerPool,
};
use prometheus_parking_lot::infra::mailbox::{InMemoryMailbox, InMemoryResultMailbox};
use prometheus_parking_lot::infra::queue::{InMemoryLeaseQueue, InMemoryQueue};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};
use prometheus_parking_lot::{DurableOptions, DurableResourcePool};

use crate::support::Probe;

/// Largest queue any workload fills.
const DEPTH: usize = 1 << 20;
/// Distinct mailbox keys of the in-memory mailbox (it keeps one message per
/// key, so cycling bounds its size across criterion iterations).
const KEY_SPACE: u64 = 4_096;
/// Budget of every task: far above any service time here.
const BUDGET: Duration = Duration::from_secs(60);

/// Which pool a harness drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `ResourcePool` with `InMemoryQueue` and `InMemoryMailbox`.
    Resource,
    /// Native `WorkerPool` with one OS thread per unit.
    Worker,
    /// `DurableResourcePool` on `InMemoryLeaseQueue` and
    /// `InMemoryResultMailbox`: the durable dispatch path without storage.
    DurableMemory,
    /// `DurableResourcePool` on redb (`RedbLeaseQueue`, `RedbResultMailbox`)
    /// in a temporary directory.
    #[cfg(feature = "embedded")]
    DurableRedb,
}

impl Kind {
    /// Every pool this build can run.
    pub const ALL: &[Self] = &[
        Self::Resource,
        Self::Worker,
        Self::DurableMemory,
        #[cfg(feature = "embedded")]
        Self::DurableRedb,
    ];

    /// The name used in benchmark ids and reports.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Resource => "resource_pool",
            Self::Worker => "worker_pool",
            Self::DurableMemory => "durable_memory",
            #[cfg(feature = "embedded")]
            Self::DurableRedb => "durable_redb",
        }
    }

    /// True for the durable pools (slower per task; smaller workloads).
    pub const fn is_durable(self) -> bool {
        !matches!(self, Self::Resource | Self::Worker)
    }
}

/// The executor of every pool.
#[derive(Clone)]
pub struct Work {
    probe: Arc<Probe>,
    service: Duration,
}

impl Work {
    async fn run(&self, id: u64) -> u64 {
        self.probe.started(id);
        if !self.service.is_zero() {
            tokio::time::sleep(self.service).await;
        }
        self.probe.ended(id);
        id
    }
}

#[async_trait]
impl TaskExecutor<u64, u64> for Work {
    async fn execute(&self, id: u64, _meta: TaskMetadata) -> Result<u64, TaskError> {
        Ok(self.run(id).await)
    }
}

#[async_trait]
impl WorkerExecutor<u64, u64> for Work {
    async fn execute(&self, id: u64, _meta: TaskMetadata) -> Result<u64, TaskError> {
        Ok(self.run(id).await)
    }
}

/// `ResourcePool`'s mailbox: stores in an `InMemoryMailbox`, then stamps the
/// task (its id is the key's `user_id`).
pub struct ProbeMailbox {
    inner: InMemoryMailbox<u64>,
    probe: Arc<Probe>,
}

impl Mailbox<u64> for ProbeMailbox {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<u64>,
    ) -> Result<(), SchedulerError> {
        self.inner.deliver(key, status, payload)?;
        let id = key
            .user_id
            .as_deref()
            .and_then(|id| id.parse().ok())
            .unwrap_or(u64::MAX);
        self.probe.finished(id);
        Ok(())
    }
}

/// A durable pool's result mailbox: delegates, then stamps the task once the
/// inner store resolved.
pub struct ProbeResultMailbox<M> {
    inner: M,
    probe: Arc<Probe>,
}

impl<M: ResultMailbox<u64>> ResultMailbox<u64> for ProbeResultMailbox<M> {
    async fn deliver(&self, delivery: Delivery<u64>) -> Result<DeliveryOutcome, SchedulerError> {
        let id = delivery.task_id;
        let outcome = self.inner.deliver(delivery).await?;
        self.probe.finished(id);
        Ok(outcome)
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<u64>>, SchedulerError>> + Send {
        self.inner.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<u64>>, SchedulerError>> + Send {
        self.inner.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.inner.evict_expired()
    }
}

type Resource = ResourcePool<u64, u64, InMemoryQueue<u64>, ProbeMailbox, Work, TokioSpawner>;
type DurableMemory = DurableResourcePool<
    u64,
    u64,
    InMemoryLeaseQueue<u64>,
    ProbeResultMailbox<InMemoryResultMailbox<u64>>,
    Work,
    TokioSpawner,
>;
#[cfg(feature = "embedded")]
type DurableRedb = DurableResourcePool<
    u64,
    u64,
    prometheus_parking_lot::infra::RedbLeaseQueue<u64>,
    ProbeResultMailbox<prometheus_parking_lot::infra::RedbResultMailbox<u64>>,
    Work,
    TokioSpawner,
>;

enum Pool {
    Resource(Resource),
    Worker(Arc<WorkerPool<u64, u64, Work>>),
    DurableMemory(DurableMemory),
    #[cfg(feature = "embedded")]
    DurableRedb {
        pool: DurableRedb,
        /// Removed (with the store file) when the harness is dropped.
        _dir: TempDir,
    },
}

/// One pool, its probe, and the uniform submit call.
pub struct Harness {
    pool: Pool,
    probe: Arc<Probe>,
}

const fn limits(units: u32) -> PoolLimits {
    PoolLimits {
        max_units: units,
        max_queue_depth: DEPTH,
        default_timeout: BUDGET,
    }
}

fn options() -> DurableOptions {
    DurableOptions::new(Duration::from_secs(30)).with_name("bench")
}

impl Harness {
    /// Build a pool of `units` units whose tasks hold their units for
    /// `service`. Call inside the benchmark runtime (the durable pools spawn
    /// their dispatcher on it).
    pub fn new(kind: Kind, probe: Arc<Probe>, service: Duration, units: u32) -> Self {
        let work = Work {
            probe: Arc::clone(&probe),
            service,
        };
        let spawner = TokioSpawner::new(tokio::runtime::Handle::current());
        let pool = match kind {
            Kind::Resource => Pool::Resource(ResourcePool::new(
                limits(units),
                InMemoryQueue::new(DEPTH),
                ProbeMailbox {
                    inner: InMemoryMailbox::new(),
                    probe: Arc::clone(&probe),
                },
                work,
                spawner,
            )),
            Kind::Worker => {
                let config = WorkerPoolConfig::new()
                    .with_worker_count(units as usize)
                    .with_max_units(units)
                    .with_max_queue_depth(DEPTH)
                    .with_timeout_ms(u64::try_from(BUDGET.as_millis()).unwrap_or(u64::MAX));
                Pool::Worker(Arc::new(
                    WorkerPool::new(config, work).expect("valid worker pool config"),
                ))
            }
            Kind::DurableMemory => Pool::DurableMemory(
                DurableResourcePool::start(
                    limits(units),
                    InMemoryLeaseQueue::new(DEPTH),
                    ProbeResultMailbox {
                        inner: InMemoryResultMailbox::new(),
                        probe: Arc::clone(&probe),
                    },
                    work,
                    spawner,
                    options(),
                )
                .expect("valid durable options"),
            ),
            #[cfg(feature = "embedded")]
            Kind::DurableRedb => {
                let dir = tempfile::tempdir().expect("temporary directory");
                let store = prometheus_parking_lot::infra::EmbeddedStore::open(
                    dir.path().join("bench.redb"),
                )
                .expect("open the redb store");
                let pool = DurableResourcePool::start(
                    limits(units),
                    store.lease_queue(DEPTH),
                    ProbeResultMailbox {
                        inner: store.result_mailbox(),
                        probe: Arc::clone(&probe),
                    },
                    work,
                    spawner,
                    options(),
                )
                .expect("valid durable options");
                Pool::DurableRedb { pool, _dir: dir }
            }
        };
        Self { pool, probe }
    }

    /// Stamp the submission of `id` and submit it at `cost` units.
    ///
    /// # Panics
    ///
    /// When the pool refuses the task (no workload here should be refused).
    pub async fn submit(&self, id: u64, cost: u32) {
        let meta = |key: u64| TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: "bench".into(),
                user_id: Some(key.to_string()),
                session_id: None,
            }),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, cost),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        };
        self.probe.submitted(id);
        match &self.pool {
            Pool::Resource(pool) => {
                let task = ScheduledTask {
                    meta: meta(id % KEY_SPACE),
                    payload: id,
                };
                pool.submit(task).await.expect("resource pool accepts");
            }
            Pool::Worker(pool) => {
                let key = pool.submit(id, meta(id)).expect("worker pool accepts");
                let pool = Arc::clone(pool);
                let probe = Arc::clone(&self.probe);
                tokio::spawn(async move {
                    pool.retrieve_async(&key, BUDGET)
                        .await
                        .expect("the result arrives");
                    probe.finished(id);
                });
            }
            Pool::DurableMemory(pool) => {
                let task = ScheduledTask {
                    meta: meta(id),
                    payload: id,
                };
                pool.submit(task).await.expect("durable pool accepts");
            }
            #[cfg(feature = "embedded")]
            Pool::DurableRedb { pool, .. } => {
                let task = ScheduledTask {
                    meta: meta(id),
                    payload: id,
                };
                pool.submit(task).await.expect("durable pool accepts");
            }
        }
    }
}
