//! Scenario S-9 (cr-29) for the durable pool: a shutdown loses no accepted
//! task, and every task it hands back runs after a restart.
//!
//! The first "process" — a thread with its own tokio runtime — opens the
//! backend, starts a `DurableResourcePool` with three units and submits six
//! one-unit-or-more tasks in this order:
//!
//! - two long tasks (one unit each), which start;
//! - one task costing two units, leased but left waiting for capacity (it
//!   blocks further leasing);
//! - three short tasks, never leased.
//!
//! It then calls `shutdown_with` under one policy, checks the report, and
//! exits: its runtime, and with it every in-memory trace of the pool, is
//! dropped. A second pool on the same backend (redb reopened from its file,
//! Postgres reconnected) must then run every task that was not completed,
//! leave exactly one `Completed` result per task, and drain the queue.
//!
//! Postgres tests need Docker and are `#[ignore]`d; they use
//! `PARKING_LOT_PG_MAJOR` (default 18). Run with:
//!
//! ```text
//! cargo nextest run --workspace --all-features --locked --run-ignored all -E "test(shutdown_)"
//! ```

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::core::shutdown::ShutdownReport;
use prometheus_parking_lot::core::{
    LeaseQueue, ResultMailbox, ScheduledTask, ShutdownPolicy, TaskError, TaskExecutor,
    TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind, now_ms};
use prometheus_parking_lot::{DurableOptions, DurableResourcePool};

use crate::support::{eventually, key, limits, spawner};

/// Short, so a lease left to expire is redelivered quickly.
const LEASE: Duration = Duration::from_secs(2);
const TASKS: [&str; 6] = ["long-1", "long-2", "big", "short-1", "short-2", "short-3"];
/// The drain deadline.
const DRAIN: Duration = Duration::from_millis(300);

fn options() -> DurableOptions {
    DurableOptions::new(LEASE)
        .with_name("s9")
        .with_poll_interval(Duration::from_millis(50))
}

/// Phase one runs `long-*` for `long`, everything else at once; phase two
/// (`long == ZERO`) runs everything at once. Counts the executions that
/// returned.
#[derive(Clone)]
struct Executor {
    long: Duration,
    returned: Arc<AtomicU32>,
}

impl Executor {
    fn new(long: Duration) -> Self {
        Self {
            long,
            returned: Arc::new(AtomicU32::new(0)),
        }
    }
}

#[async_trait]
impl TaskExecutor<String, String> for Executor {
    async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        if payload.starts_with("long") {
            tokio::time::sleep(self.long).await;
        }
        self.returned.fetch_add(1, Ordering::SeqCst);
        Ok(format!("done:{payload}"))
    }
}

fn task(name: &str) -> ScheduledTask<String> {
    let units = if name == "big" { 2 } else { 1 };
    ScheduledTask {
        meta: TaskMetadata {
            id: 1,
            mailbox: Some(key(name)),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, units),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: name.to_owned(),
    }
}

/// Opens the durable backend; called once per "process".
trait Backend: Send + Sync + 'static {
    type Queue: LeaseQueue<String> + 'static;
    type Mailbox: ResultMailbox<String> + 'static;

    fn open(&self) -> impl Future<Output = (Self::Queue, Self::Mailbox)> + Send;
}

/// What the first process saw.
#[derive(Debug)]
struct PhaseOne {
    report: ShutdownReport,
    /// Executions that returned (and so were delivered and acked).
    returned: u32,
}

/// The first process: submit, shut down under `policy`, exit. Runs on its own
/// thread and runtime so nothing of it survives into the restart.
fn first_process<B: Backend>(backend: &Arc<B>, policy: ShutdownPolicy, long: Duration) -> PhaseOne {
    let backend = Arc::clone(backend);
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("first process runtime");
        let phase = runtime.block_on(async move {
            let (queue, mailbox) = backend.open().await;
            let executor = Executor::new(long);
            let pool = DurableResourcePool::start(
                limits(3),
                queue,
                mailbox,
                executor.clone(),
                spawner(),
                options(),
            )
            .expect("start the first pool");
            for name in TASKS {
                assert_eq!(
                    pool.submit(task(name)).await.expect("accepted"),
                    TaskStatus::Queued
                );
            }
            // Both long tasks running and `big` leased, waiting for capacity.
            let pool_ref = &pool;
            let settled = eventually(Duration::from_secs(20), || async move {
                let depth = pool_ref.queue().depth().await.expect("depth");
                (pool_ref.running_tasks() == 2 && depth.leased == 3).then_some(())
            })
            .await;
            assert!(settled.is_some(), "the pool never reached its start state");

            let mut report = pool.shutdown_with(policy.clone()).await;
            if matches!(policy, ShutdownPolicy::Drain { .. }) {
                assert_eq!(
                    report.unsettled, 2,
                    "the long tasks outlive the drain: {report:?}"
                );
                // A second call waits for them.
                let rest = pool
                    .shutdown_with(ShutdownPolicy::Drain {
                        deadline: Duration::from_secs(30),
                    })
                    .await;
                assert_eq!(rest.unsettled, 0, "{rest:?}");
                report.completed += rest.completed;
                report.unsettled = 0;
            }
            PhaseOne {
                report,
                returned: executor.returned.load(Ordering::SeqCst),
            }
        });
        // Dropping the runtime drops every task of the first pool: the
        // in-memory state is gone, as after a process exit.
        drop(runtime);
        phase
    })
    .join()
    .expect("first process")
}

/// The restarted process: a new pool on the same backend runs every task
/// the first one did not complete; exactly one result per task.
async fn restart_and_check<B: Backend>(backend: &B, first: &PhaseOne) {
    let (queue, mailbox) = backend.open().await;
    let executor = Executor::new(Duration::ZERO);
    let pool = DurableResourcePool::start(
        limits(3),
        queue,
        mailbox,
        executor.clone(),
        spawner(),
        options(),
    )
    .expect("start the restarted pool");
    let pool_ref = &pool;
    for name in TASKS {
        let key = key(name);
        let key_ref = &key;
        let entry = eventually(LEASE + Duration::from_secs(30), || async move {
            pool_ref.mailbox().consume(key_ref).await.expect("consume")
        })
        .await
        .unwrap_or_else(|| panic!("no result for {name}: the accepted task was lost"));
        assert_eq!(entry.status, TaskStatus::Completed, "{name}");
        assert_eq!(entry.payload, Some(format!("done:{name}")), "{name}");
    }
    let drained = eventually(Duration::from_secs(10), || async move {
        let depth = pool_ref.queue().depth().await.expect("depth");
        (depth.live() == 0).then_some(depth)
    })
    .await;
    assert!(drained.is_some(), "the queue never drained");
    let depth = pool.queue().depth().await.expect("depth");
    assert_eq!(depth.dead_lettered, 0, "{depth:?}");
    for name in TASKS {
        assert!(
            pool.mailbox()
                .consume(&key(name))
                .await
                .expect("consume")
                .is_none(),
            "a second result for {name}"
        );
    }
    let total = u32::try_from(TASKS.len()).expect("six");
    assert_eq!(
        first.returned + executor.returned.load(Ordering::SeqCst),
        total,
        "every task returned exactly once across the two processes ({first:?})"
    );
    pool.shutdown_with(ShutdownPolicy::CancelImmediately).await;
}

/// The first process's report for `policy`: `big` is always handed back;
/// the long tasks complete under `Drain` and are handed back otherwise.
fn assert_first_report(policy: &ShutdownPolicy, first: &PhaseOne) {
    let report = &first.report;
    if policy.cancels_stragglers() {
        assert_eq!(
            (report.completed, report.cancelled, report.requeued),
            (0, 3, 3),
            "{report:?}"
        );
        assert_eq!(first.returned, 0, "{first:?}");
    } else {
        assert_eq!(
            (report.completed, report.cancelled, report.requeued),
            (2, 1, 1),
            "{report:?}"
        );
        assert_eq!(first.returned, 2, "{first:?}");
    }
    assert_eq!(
        (
            report.recorded,
            report.dropped,
            report.unanswered,
            report.unsettled
        ),
        (0, 0, 0, 0),
        "{report:?}"
    );
}

async fn shut_down_and_restart<B: Backend>(backend: B, policy: ShutdownPolicy) {
    let long = if policy.cancels_stragglers() {
        Duration::from_secs(600)
    } else {
        Duration::from_millis(1_500)
    };
    let backend = Arc::new(backend);
    let first = {
        let backend = Arc::clone(&backend);
        let policy = policy.clone();
        tokio::task::spawn_blocking(move || first_process(&backend, policy, long))
            .await
            .expect("first process")
    };
    assert_first_report(&policy, &first);
    restart_and_check(backend.as_ref(), &first).await;
}

const fn drain() -> ShutdownPolicy {
    ShutdownPolicy::Drain { deadline: DRAIN }
}

const fn drain_then_cancel() -> ShutdownPolicy {
    ShutdownPolicy::DrainThenCancel { deadline: DRAIN }
}

#[cfg(feature = "embedded")]
mod redb {
    use std::path::PathBuf;

    use prometheus_parking_lot::core::ShutdownPolicy;
    use prometheus_parking_lot::infra::queue::RedbLeaseQueue;
    use prometheus_parking_lot::infra::{EmbeddedStore, RedbResultMailbox};

    use super::{Backend, drain, drain_then_cancel, shut_down_and_restart};

    /// A redb file in a temporary directory, opened afresh by each process.
    struct Redb {
        file: PathBuf,
        _dir: tempfile::TempDir,
    }

    impl Redb {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            Self {
                file: dir.path().join("s9.redb"),
                _dir: dir,
            }
        }
    }

    impl Backend for Redb {
        type Queue = RedbLeaseQueue<String>;
        type Mailbox = RedbResultMailbox<String>;

        async fn open(&self) -> (Self::Queue, Self::Mailbox) {
            let store = EmbeddedStore::open(&self.file).expect("open the store");
            (store.lease_queue::<String>(100), store.result_mailbox())
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_durable_redb_drain_restart_runs_the_rest() {
        shut_down_and_restart(Redb::new(), drain()).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_durable_redb_drain_then_cancel_requeues_and_restart_runs_them() {
        shut_down_and_restart(Redb::new(), drain_then_cancel()).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_durable_redb_cancel_immediately_requeues_and_restart_runs_them() {
        shut_down_and_restart(Redb::new(), ShutdownPolicy::CancelImmediately).await;
    }
}

#[cfg(feature = "postgres")]
mod postgres {
    use prometheus_parking_lot::core::ShutdownPolicy;
    use prometheus_parking_lot::infra::mailbox::PgResultMailbox;
    use prometheus_parking_lot::infra::queue::PgLeaseQueue;

    use super::{Backend, drain, drain_then_cancel, shut_down_and_restart};
    use crate::support::pg::{Pg18, connect};

    /// Queue and mailbox name.
    const NAME: &str = "s9";

    /// A database URL; each process opens its own connection pool.
    struct Postgres {
        url: String,
    }

    impl Backend for Postgres {
        type Queue = PgLeaseQueue<String>;
        type Mailbox = PgResultMailbox<String>;

        async fn open(&self) -> (Self::Queue, Self::Mailbox) {
            let pool = connect(&self.url).await;
            (
                PgLeaseQueue::<String>::new(pool.clone(), NAME, 100),
                PgResultMailbox::<String>::new(pool, NAME),
            )
        }
    }

    async fn run(policy: ShutdownPolicy) {
        let pg = Pg18::start().await;
        let pool = connect(&pg.url).await;
        PgLeaseQueue::<String>::migrate(&pool)
            .await
            .expect("migrate queue");
        PgResultMailbox::<String>::migrate(&pool)
            .await
            .expect("migrate mailbox");
        shut_down_and_restart(
            Postgres {
                url: pg.url.clone(),
            },
            policy,
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg container (PARKING_LOT_PG_MAJOR, default 18)"]
    async fn shutdown_durable_pg_drain_restart_runs_the_rest() {
        run(drain()).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg container (PARKING_LOT_PG_MAJOR, default 18)"]
    async fn shutdown_durable_pg_drain_then_cancel_requeues_and_restart_runs_them() {
        run(drain_then_cancel()).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg container (PARKING_LOT_PG_MAJOR, default 18)"]
    async fn shutdown_durable_pg_cancel_immediately_requeues_and_restart_runs_them() {
        run(ShutdownPolicy::CancelImmediately).await;
    }
}
