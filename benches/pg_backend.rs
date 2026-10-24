//! OA-17 fixed real-Postgres workload, shared byte-for-byte with baseline0.3.
//!
//! This harness=false executable emits one raw process sample. Startup,
//! migrations, fixed warm-up, verification and shutdown are outside the timed
//! phase, but included in the runner's full-process peak RSS. No schema is
//! relabeled: each binary migrates its own fresh disposable database. Compare
//! only Stored, never match Duplicate's version-dependent payload shape.
//! See pg_measure.py for paired sampling, provenance and report validation.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use prometheus_parking_lot::core::{
    Delivery, DeliveryOutcome, LeaseQueue, PoolLimits, ResultMailbox, ScheduledTask,
    ShutdownPolicy, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::PgPool;
use prometheus_parking_lot::infra::mailbox::PgResultMailbox;
use prometheus_parking_lot::infra::queue::PgLeaseQueue;
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind};
use prometheus_parking_lot::{DurableOptions, DurableResourcePool};
use serde_json::json;
use tokio::sync::Barrier;

const OPERATIONS: usize = 1024;
const WARMUP: usize = 128;
const PAYLOAD_BYTES: usize = 256;
const RUNTIME_THREADS: usize = 4;
const MAX_UNITS: u32 = 16;
const LEASE: Duration = Duration::from_secs(30);
const POLL: Duration = Duration::from_millis(1);
const NAME: &str = "oa17";
const HOT_ID: u64 = 999_999;

struct Executor;

#[async_trait]
impl TaskExecutor<String, String> for Executor {
    async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        Ok(payload)
    }
}

type Durable = DurableResourcePool<
    String,
    String,
    PgLeaseQueue<String>,
    PgResultMailbox<String>,
    Executor,
    TokioSpawner,
>;

struct Work {
    case: String,
    queue: PgLeaseQueue<String>,
    mailbox: PgResultMailbox<String>,
    durable: Option<Durable>,
    payload: String,
}

fn address(client: usize) -> MailboxKey {
    MailboxKey {
        tenant: "oa17-tenant".to_owned(),
        user_id: None,
        session_id: Some(format!("client-{client}")),
    }
}

fn task(client: usize, id: u64, payload: &str) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(address(client)),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: Some(format!("oa17-task-{id}")),
            sequence: id,
        },
        payload: payload.to_owned(),
    }
}

impl Work {
    async fn one(&self, client: usize, id: u64) -> u64 {
        match self.case.as_str() {
            "queue" => {
                self.queue
                    .enqueue(task(client, id, &self.payload))
                    .await
                    .expect("enqueue");
                let leased = loop {
                    if let Some(leased) = self.queue.lease(1, LEASE).await.expect("lease").pop() {
                        break leased;
                    }
                    tokio::time::sleep(POLL).await;
                };
                assert_eq!(leased.task.payload, self.payload);
                let observed = leased.task.meta.id;
                self.queue.ack(leased.token).await.expect("ack");
                observed
            }
            "mailbox_unique" => {
                let key = address(client);
                let delivery = Delivery::new(
                    key.clone(),
                    id,
                    TaskStatus::Completed,
                    Some(self.payload.clone()),
                )
                .with_idempotency_key(format!("oa17-result-{id}"));
                assert_eq!(
                    self.mailbox.deliver(delivery).await.expect("deliver"),
                    DeliveryOutcome::Stored
                );
                let peeked = self
                    .mailbox
                    .peek(&key)
                    .await
                    .expect("peek")
                    .expect("visible result");
                assert_eq!(peeked.task_id, id);
                let taken = self
                    .mailbox
                    .consume(&key)
                    .await
                    .expect("consume")
                    .expect("one result");
                assert_eq!(taken, peeked);
                assert_eq!(taken.status, TaskStatus::Completed);
                assert_eq!(taken.payload.as_deref(), Some(self.payload.as_str()));
                id
            }
            "mailbox_duplicate" => {
                let delivery = Delivery::new(
                    address(0),
                    id,
                    TaskStatus::Completed,
                    Some("retry must not replace the original".to_owned()),
                )
                .with_idempotency_key("oa17-hot-key");
                // Compatible with the 0.3 unit Duplicate and OA-07 metadata.
                assert_ne!(
                    self.mailbox.deliver(delivery).await.expect("duplicate"),
                    DeliveryOutcome::Stored
                );
                id
            }
            "durable" => {
                let pool = self.durable.as_ref().expect("durable pool");
                assert_eq!(
                    pool.submit(task(client, id, &self.payload))
                        .await
                        .expect("submit"),
                    TaskStatus::Queued
                );
                let key = address(client);
                let result = loop {
                    if let Some(entry) = self.mailbox.consume(&key).await.expect("durable consume")
                    {
                        break entry;
                    }
                    tokio::time::sleep(POLL).await;
                };
                assert_eq!(result.task_id, id);
                assert_eq!(result.status, TaskStatus::Completed);
                assert_eq!(result.payload.as_deref(), Some(self.payload.as_str()));
                id
            }
            other => panic!("unsupported workload {other}"),
        }
    }
}

async fn phase(
    work: &Arc<Work>,
    clients: usize,
    start_id: usize,
    count: usize,
    delay_us: u64,
) -> (Vec<u64>, u64) {
    let barrier = Arc::new(Barrier::new(clients + 1));
    let mut workers = Vec::new();
    for client in 0..clients {
        let work = Arc::clone(work);
        let barrier = Arc::clone(&barrier);
        workers.push(tokio::spawn(async move {
            let mut observations = Vec::new();
            barrier.wait().await;
            for offset in (client..count).step_by(clients) {
                let id = u64::try_from(start_id + offset).expect("bounded task id");
                let before = Instant::now();
                let seen = tokio::time::timeout(Duration::from_secs(120), work.one(client, id))
                    .await
                    .expect("operation watchdog, never a skipped measurement");
                // Explicit, recorded report-classifier control only. Normal
                // samples require zero; never hide this in workload identity.
                if delay_us != 0 {
                    tokio::time::sleep(Duration::from_micros(delay_us)).await;
                }
                let ns = u64::try_from(before.elapsed().as_nanos()).expect("bounded latency");
                observations.push((seen, ns));
            }
            observations
        }));
    }
    let before = Instant::now();
    barrier.wait().await;
    let mut all = Vec::new();
    for worker in workers {
        all.extend(worker.await.expect("client task"));
    }
    let elapsed = u64::try_from(before.elapsed().as_nanos()).expect("bounded sample duration");
    all.sort_unstable_by_key(|(id, _)| *id);
    let expected: Vec<u64> = (start_id..start_id + count)
        .map(|id| u64::try_from(id).expect("bounded id"))
        .collect();
    assert_eq!(
        all.iter().map(|(id, _)| *id).collect::<Vec<_>>(),
        expected,
        "every operation is observed exactly once"
    );
    (all.into_iter().map(|(_, ns)| ns).collect(), elapsed)
}

async fn sample(case: String, clients: usize, delay_us: u64) -> serde_json::Value {
    let url = std::env::var("OA17_DATABASE_URL").expect("disposable database URL is required");
    let deadline = Instant::now() + Duration::from_secs(60);
    let connection = loop {
        match PgPool::connect(&url).await {
            Ok(pool) => break pool,
            Err(error) if Instant::now() >= deadline => panic!("database startup: {error}"),
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    };
    let max_connections = connection.options().get_max_connections();
    PgLeaseQueue::<String>::migrate(&connection)
        .await
        .expect("queue migration");
    PgResultMailbox::<String>::migrate(&connection)
        .await
        .expect("mailbox migration");
    let queue = PgLeaseQueue::new(connection.clone(), NAME, 1_000_000);
    let mailbox = PgResultMailbox::new(connection.clone(), NAME);
    let durable = (case == "durable").then(|| {
        DurableResourcePool::start(
            PoolLimits {
                max_units: MAX_UNITS,
                max_queue_depth: 1_000_000,
                default_timeout: Duration::from_secs(120),
            },
            PgLeaseQueue::new(connection.clone(), NAME, 1_000_000),
            PgResultMailbox::new(connection, NAME),
            Executor,
            TokioSpawner::new(tokio::runtime::Handle::current()),
            DurableOptions::new(LEASE)
                .with_name(NAME)
                .with_poll_interval(POLL),
        )
        .expect("durable pool")
    });
    let work = Arc::new(Work {
        case: case.clone(),
        queue,
        mailbox,
        durable,
        payload: "x".repeat(PAYLOAD_BYTES),
    });
    if case == "mailbox_duplicate" {
        assert_eq!(
            work.mailbox
                .deliver(
                    Delivery::new(
                        address(0),
                        HOT_ID,
                        TaskStatus::Completed,
                        Some(work.payload.clone())
                    )
                    .with_idempotency_key("oa17-hot-key")
                )
                .await
                .expect("seed hot key"),
            DeliveryOutcome::Stored
        );
    }
    let _warmup = phase(&work, clients, 0, WARMUP, 0).await;
    let (latencies_ns, elapsed_ns) = phase(&work, clients, WARMUP, OPERATIONS, delay_us).await;
    if let Some(pool) = &work.durable {
        let report = pool
            .shutdown_with(ShutdownPolicy::Drain {
                deadline: Duration::from_secs(60),
            })
            .await;
        assert_eq!(report.unsettled, 0, "durable accounting drains");
    }
    if case == "queue" || case == "durable" {
        let depth = work.queue.depth().await.expect("queue depth");
        assert_eq!((depth.live(), depth.dead_lettered), (0, 0), "{depth:?}");
    }
    for client in 0..clients {
        if case == "mailbox_duplicate" && client == 0 {
            let original = work
                .mailbox
                .consume(&address(0))
                .await
                .expect("consume hot key")
                .expect("one original result");
            assert_eq!(original.task_id, HOT_ID);
            assert_eq!(original.status, TaskStatus::Completed);
            assert_eq!(original.payload.as_deref(), Some(work.payload.as_str()));
        }
        assert!(
            work.mailbox
                .consume(&address(client))
                .await
                .expect("no extra result")
                .is_none()
        );
    }
    json!({
        "schema": 1,
        "workload": { "case": case, "clients": clients, "operations": OPERATIONS,
            "warmup": WARMUP, "payload_bytes": PAYLOAD_BYTES, "runtime_threads": RUNTIME_THREADS,
            "max_units": MAX_UNITS, "queue_max_depth": 1_000_000, "lease_ms": 30_000,
            "poll_ms": 1, "priority": "Normal", "cost_units": 1,
            "sqlx_max_connections": max_connections, "seed": "sequential-ids-v1" },
        "control_delay_us": delay_us, "elapsed_ns": elapsed_ns, "latencies_ns": latencies_ns,
        "latency_scope": "client cycle including storage calls, checks, polling and explicit control delay",
        "verified_operations": OPERATIONS,
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    assert!(
        args.len() == 4 && args[0] == "--case" && args[2] == "--clients",
        "expected --case CASE --clients 1|8; run the built binary via pg_measure.py"
    );
    let case = args[1].clone();
    assert!(["queue", "mailbox_unique", "mailbox_duplicate", "durable"].contains(&case.as_str()));
    let clients: usize = args[3].parse().expect("clients");
    assert!([1, 8].contains(&clients));
    let delay_us: u64 = std::env::var("OA17_CONTROL_DELAY_US")
        .unwrap_or_else(|_| "0".to_owned())
        .parse()
        .expect("control delay");
    assert!(
        [0, 100_000].contains(&delay_us),
        "only the declared slow control is supported"
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(RUNTIME_THREADS)
        .enable_all()
        .build()
        .expect("benchmark runtime");
    println!("{}", runtime.block_on(sample(case, clients, delay_us)));
}
