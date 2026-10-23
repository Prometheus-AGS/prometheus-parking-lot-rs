//! Real-Postgres tests for [`PgLeaseQueue`] (cr-26) on the PG 18 project image.
//!
//! Every test needs Docker, so each is `#[ignore]`d and compiles in CI without
//! it. Run them with:
//!
//! ```text
//! cargo test -p prometheus-parking-lot-pg --test lease_queue -- --ignored
//! ```
//!
//! - the shared backend conformance suite (ADR 0003), one test per case;
//! - `sigkill_redelivery` (scenario S-5): a child process leases a task and is
//!   killed with `SIGKILL` mid-task; the task is redelivered after the lease
//!   expires, the dead consumer's token is fenced off, and exactly one ack
//!   completes it;
//! - `concurrent_dispatchers` (scenario S-6): eight dispatchers drain 10 000
//!   tasks and no task is leased twice;
//! - wake-up, reaper, max-attempts and undecodable-payload behaviour.
//!
//! Negative controls (`RUSTFLAGS='--cfg parking_lot_negative_control="s-5"'`,
//! `"s-6"`, `"s-5-fencing"`) break the library on purpose; the tests above must
//! then fail.

mod support;

use std::collections::HashSet;
use std::future::Future;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use prometheus_parking_lot_core::conformance::QueueHarness;
use prometheus_parking_lot_core::{
    Enqueued, LeaseQueue, LeaseToken, NackAction, Priority, QueueEntryId, ResourceCost,
    ResourceKind, ScheduledTask, SchedulerError, TaskMetadata, lease_queue_conformance,
};
use prometheus_parking_lot_pg::{PgLeaseQueue, Wake};
use sqlx::postgres::PgPoolOptions;
use support::{PgContainer, TestResult};

/// Postgres major version under test. The 17/18 matrix is cr-36.
const PG_MAJOR: u8 = 18;

fn task(priority: Priority, payload: &str, key: Option<&str>) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id: 0,
            mailbox: None,
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: key.map(str::to_owned),
            sequence: 0,
        },
        payload: payload.to_owned(),
    }
}

/// A container with the schema applied.
async fn start() -> TestResult<PgContainer> {
    let pg = PgContainer::start(PG_MAJOR).await?;
    PgLeaseQueue::<String>::migrate(&pg.pool).await?;
    Ok(pg)
}

// ── conformance ─────────────────────────────────────────────────────────────

/// Builds a fresh queue name per queue; time moves by really sleeping, because
/// the queue reads the database clock.
struct PgQueueHarness {
    pg: PgContainer,
    next: AtomicU32,
}

impl PgQueueHarness {
    async fn start() -> Self {
        Self {
            pg: start().await.expect("start Postgres and migrate"),
            next: AtomicU32::new(0),
        }
    }
}

impl QueueHarness for PgQueueHarness {
    type Queue = PgLeaseQueue<String>;

    fn queue(&self, max_depth: usize) -> impl Future<Output = Self::Queue> + Send {
        let name = format!("conformance-{}", self.next.fetch_add(1, Ordering::Relaxed));
        std::future::ready(PgLeaseQueue::new(self.pg.pool.clone(), name, max_depth))
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(by)
    }

    fn lease_duration(&self) -> Duration {
        Duration::from_secs(3)
    }
}

mod conformance {
    use super::*;

    lease_queue_conformance!(
        test_attr = #[cfg_attr(all(), tokio::test, ignore = "needs Docker: starts a parking-lot-pg container")],
        harness = PgQueueHarness::start().await,
    );
}

// ── S-5: SIGKILL redelivery ─────────────────────────────────────────────────

const CHILD_URL: &str = "PLQ_PG_CHILD_URL";
const CHILD_QUEUE: &str = "PLQ_PG_CHILD_QUEUE";
const CHILD_LEASE: Duration = Duration::from_secs(4);

/// Entry point of the consumer child process; a no-op in a normal test run.
///
/// It leases one task, reports the lease on stdout and then "works" forever;
/// the parent kills it with `SIGKILL` while it holds the lease.
#[tokio::test]
async fn sigkill_child_entry() {
    let (Ok(url), Ok(queue)) = (std::env::var(CHILD_URL), std::env::var(CHILD_QUEUE)) else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("child: connect");
    let queue = PgLeaseQueue::<String>::new(pool, queue, 100);
    let leased = queue.lease(1, CHILD_LEASE).await.expect("child: lease");
    let Some(leased) = leased.into_iter().next() else {
        println!("NOTHING");
        return;
    };
    let token = serde_json::to_string(&leased.token).expect("token json");
    println!("LEASED {} {} {token}", leased.task.payload, leased.attempt);
    println!("WORKING");
    let _ = std::io::stdout().flush();
    loop {
        std::thread::park();
    }
}

/// What the killed child reported.
struct KilledChild {
    payload: String,
    attempt: u32,
    token: LeaseToken,
}

/// Run the consumer in a child process and `SIGKILL` it once it is mid-task.
fn run_and_kill_consumer(url: &str, queue: &str) -> KilledChild {
    let mut child = Command::new(std::env::current_exe().expect("current exe"))
        .args([
            "--exact",
            "sigkill_child_entry",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_URL, url)
        .env(CHILD_QUEUE, queue)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn consumer");
    let stdout = child.stdout.take().expect("child stdout");

    let mut leased = None;
    for line in BufReader::new(stdout).lines() {
        let line = line.expect("read child stdout");
        // With `--nocapture` libtest prints "test <name> ... " without a newline
        // before the test's own output, so the markers are not at line start.
        if let Some((_, rest)) = line.split_once("LEASED ") {
            let mut parts = rest.splitn(3, ' ');
            let payload = parts.next().expect("payload").to_owned();
            let attempt = parts.next().expect("attempt").parse().expect("attempt u32");
            let token = serde_json::from_str(parts.next().expect("token")).expect("token json");
            leased = Some(KilledChild {
                payload,
                attempt,
                token,
            });
        } else if line.ends_with("WORKING") {
            // SIGKILL: no destructor, flush or unwinding runs in the child.
            child.kill().expect("SIGKILL the consumer");
            break;
        } else if line.ends_with("NOTHING") {
            panic!("the consumer found nothing to lease");
        }
    }
    let status = child.wait().expect("wait for consumer");
    assert!(!status.success(), "the consumer must have been killed");
    leased.expect("the consumer reported its lease before it was killed")
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container; kills a child process"]
async fn sigkill_redelivery() -> TestResult<()> {
    let pg = start().await?;
    let queue = PgLeaseQueue::<String>::new(pg.pool.clone(), "sigkill", 100);
    let entry = queue
        .enqueue(task(Priority::Normal, "work-1", Some("job-1")))
        .await?
        .entry();

    let url = pg.url.clone();
    let killed =
        tokio::task::spawn_blocking(move || run_and_kill_consumer(&url, "sigkill")).await?;
    assert_eq!(killed.payload, "work-1");
    assert_eq!(killed.attempt, 1);
    assert_eq!(killed.token.entry(), entry);

    // The consumer is dead but its lease still holds: nobody else gets the
    // task, and the entry counts as leased.
    assert!(
        queue.lease(10, CHILD_LEASE).await?.is_empty(),
        "a task under a live lease was handed out"
    );
    let depth = queue.depth().await?;
    assert_eq!((depth.ready, depth.leased), (0, 1), "{depth:?}");

    // After the lease expires the task is redelivered as a new attempt.
    let deadline = Instant::now() + CHILD_LEASE + Duration::from_secs(10);
    let redelivered = loop {
        let mut batch = queue.lease(10, Duration::from_secs(60)).await?;
        if let Some(leased) = batch.pop() {
            break leased;
        }
        assert!(
            Instant::now() < deadline,
            "the task was never redelivered after the dead consumer's lease expired"
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    };
    assert_eq!(redelivered.task.payload, "work-1");
    assert_eq!(redelivered.token.entry(), entry);
    assert_eq!(redelivered.attempt, 2, "redelivery is a new attempt");
    assert_ne!(redelivered.token, killed.token);

    // The dead consumer's token is fenced off, however late it shows up.
    assert!(matches!(
        queue.ack(killed.token).await,
        Err(SchedulerError::LeaseLost { entry: e }) if e == entry.0
    ));
    assert!(matches!(
        queue.nack(killed.token, NackAction::requeue()).await,
        Err(SchedulerError::LeaseLost { .. })
    ));

    // Completed exactly once: one ack succeeds, a second does not, nothing is
    // left to run.
    queue.ack(redelivered.token).await?;
    assert!(matches!(
        queue.ack(redelivered.token).await,
        Err(SchedulerError::LeaseLost { .. })
    ));
    assert_eq!(queue.depth().await?.live(), 0);
    assert!(queue.lease(10, Duration::from_secs(1)).await?.is_empty());
    Ok(())
}

// ── S-6: no double claim ────────────────────────────────────────────────────

const DISPATCHERS: usize = 8;
const TASKS: usize = 10_000;
const BATCH: usize = 25;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn concurrent_dispatchers() -> TestResult<()> {
    let pg = start().await?;
    let queue = PgLeaseQueue::<String>::new(pg.pool.clone(), "dispatch", TASKS);

    // Load the queue in one statement; one task through the real enqueue path
    // proves the stored shape is the one `lease` reads.
    let first = queue
        .enqueue(task(Priority::Normal, "task-0", None))
        .await?;
    assert!(matches!(first, Enqueued::Accepted(_)));
    let meta = serde_json::to_value(task(Priority::Normal, "", None).meta)?;
    sqlx::query(
        "INSERT INTO pl_lease_queue (queue_name, priority, meta, payload)
         SELECT 'dispatch', 1, $1, to_jsonb('task-' || g) FROM generate_series(1, $2) g",
    )
    .bind(meta)
    .bind(i64::try_from(TASKS - 1)?)
    .execute(&pg.pool)
    .await?;
    assert_eq!(queue.depth().await?.ready, TASKS);

    // Eight dispatchers lease concurrently until every task has been handed
    // out. Leases are long and nothing is acked, so any task seen twice was
    // leased while another dispatcher still held it.
    let claimed = Arc::new(AtomicUsize::new(0));
    let started = Instant::now();
    let mut dispatchers = Vec::new();
    for _ in 0..DISPATCHERS {
        let queue = queue.clone();
        let claimed = Arc::clone(&claimed);
        dispatchers.push(tokio::spawn(async move {
            let mut mine = Vec::new();
            while claimed.load(Ordering::Relaxed) < TASKS {
                assert!(
                    started.elapsed() < Duration::from_secs(300),
                    "dispatchers did not drain the queue"
                );
                let batch = queue
                    .lease(BATCH, Duration::from_secs(600))
                    .await
                    .expect("lease");
                claimed.fetch_add(batch.len(), Ordering::Relaxed);
                mine.extend(batch.into_iter().map(|l| (l.token.entry(), l.attempt)));
            }
            mine
        }));
    }
    let mut all = Vec::new();
    for dispatcher in dispatchers {
        all.extend(dispatcher.await?);
    }

    let distinct: HashSet<QueueEntryId> = all.iter().map(|(entry, _)| *entry).collect();
    assert_eq!(
        distinct.len(),
        all.len(),
        "{} leases were handed out for {} distinct tasks: a task was leased twice",
        all.len(),
        distinct.len()
    );
    assert_eq!(distinct.len(), TASKS);
    assert!(
        all.iter().all(|(_, attempt)| *attempt == 1),
        "a task was leased more than once"
    );
    let depth = queue.depth().await?;
    assert_eq!((depth.ready, depth.leased), (0, TASKS), "{depth:?}");
    Ok(())
}

// ── wake-up, reaper, attempts, poison ───────────────────────────────────────

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn notify_wakes_an_idle_dispatcher() -> TestResult<()> {
    let pg = start().await?;
    let queue = PgLeaseQueue::<String>::new(pg.pool.clone(), "wake", 100);
    let other = PgLeaseQueue::<String>::new(pg.pool.clone(), "wake-other", 100);
    let mut listener = queue.listener().await?;

    assert_eq!(
        listener.wait(Duration::from_millis(300)).await?,
        Wake::TimedOut
    );

    // An enqueue to another queue does not wake this one.
    let _ = other
        .enqueue(task(Priority::Normal, "elsewhere", None))
        .await?;
    assert_eq!(
        listener.wait(Duration::from_millis(500)).await?,
        Wake::TimedOut
    );

    // An enqueue to this queue does, well inside the timeout.
    let producer = queue.clone();
    let enqueue = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        producer.enqueue(task(Priority::Normal, "mine", None)).await
    });
    let began = Instant::now();
    assert_eq!(
        listener.wait(Duration::from_secs(20)).await?,
        Wake::Notified
    );
    assert!(began.elapsed() < Duration::from_secs(5));
    let _ = enqueue.await??;

    // A notification sent while the dispatcher was busy is not lost.
    let leased = queue.lease(1, Duration::from_secs(60)).await?;
    queue.nack(leased[0].token, NackAction::requeue()).await?;
    assert_eq!(
        listener.wait(Duration::from_secs(5)).await?,
        Wake::Notified,
        "a requeue wakes dispatchers"
    );

    // A backed-off requeue does not: it is not leasable yet.
    let leased = queue.lease(1, Duration::from_secs(60)).await?;
    queue
        .nack(
            leased[0].token,
            NackAction::requeue_after(Duration::from_secs(60)),
        )
        .await?;
    assert_eq!(
        listener.wait(Duration::from_millis(500)).await?,
        Wake::TimedOut
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn reaper_returns_expired_leases_and_wakes_listeners() -> TestResult<()> {
    let pg = start().await?;
    let queue = PgLeaseQueue::<String>::new(pg.pool.clone(), "reaper", 100);
    let _ = queue.enqueue(task(Priority::Normal, "lost", None)).await?;
    let mut listener = queue.listener().await?;
    let _ = queue.lease(1, Duration::from_secs(1)).await?;
    // Wake-ups from the enqueue are already queued on the listener; drain them.
    while listener.wait(Duration::from_millis(200)).await? == Wake::Notified {}

    let state = |pool: sqlx::PgPool| async move {
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM pl_lease_queue WHERE queue_name = 'reaper'",
        )
        .fetch_one(&pool)
        .await
    };
    assert_eq!(state(pg.pool.clone()).await?, "leased");

    let _reaper = queue.spawn_reaper(Duration::from_millis(200));
    assert_eq!(
        listener.wait(Duration::from_secs(15)).await?,
        Wake::Notified,
        "the reaper wakes dispatchers when a lease expires"
    );
    assert_eq!(
        state(pg.pool.clone()).await?,
        "ready",
        "the reaper, not a lease call, returned the entry"
    );
    let again = queue.lease(1, Duration::from_secs(60)).await?;
    assert_eq!(again[0].attempt, 2);
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn exhausted_attempts_are_dead_lettered() -> TestResult<()> {
    let pg = start().await?;
    let queue = PgLeaseQueue::<String>::new(pg.pool.clone(), "poison", 100).with_max_attempts(2);
    let _ = queue
        .enqueue(task(Priority::Normal, "crash-loop", Some("crash-loop")))
        .await?;
    let lease_for = Duration::from_secs(1);

    for attempt in 1..=2 {
        let leased = queue.lease(1, lease_for).await?;
        assert_eq!(leased.len(), 1, "attempt {attempt}");
        assert_eq!(leased[0].attempt, attempt);
        tokio::time::sleep(lease_for + Duration::from_millis(500)).await; // consumer "crashes"
    }

    // The third lease finds the entry out of attempts and gives up on it.
    assert!(queue.lease(1, lease_for).await?.is_empty());
    let depth = queue.depth().await?;
    assert_eq!((depth.live(), depth.dead_lettered), (0, 1), "{depth:?}");
    let reason: String =
        sqlx::query_scalar("SELECT dead_reason FROM pl_lease_queue WHERE queue_name = 'poison'")
            .fetch_one(&pg.pool)
            .await?;
    assert!(reason.contains("max attempts"), "{reason}");

    // The reaper applies the same rule to a lease that expires on the last
    // attempt.
    let _ = queue
        .enqueue(task(Priority::Normal, "second", None))
        .await?;
    for _ in 0..2 {
        let _ = queue.lease(1, lease_for).await?;
        tokio::time::sleep(lease_for + Duration::from_millis(500)).await;
        let _ = queue.reap_expired().await?;
    }
    let depth = queue.depth().await?;
    assert_eq!((depth.live(), depth.dead_lettered), (0, 2), "{depth:?}");
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn undecodable_entry_does_not_wedge_the_queue() -> TestResult<()> {
    let pg = start().await?;
    let queue = PgLeaseQueue::<String>::new(pg.pool.clone(), "poison-payload", 100);
    let meta = serde_json::to_value(task(Priority::Critical, "", None).meta)?;
    // A critical entry whose payload is not a string sorts ahead of the good one.
    sqlx::query(
        "INSERT INTO pl_lease_queue (queue_name, priority, meta, payload)
         VALUES ('poison-payload', 3, $1, '42'::jsonb)",
    )
    .bind(meta)
    .execute(&pg.pool)
    .await?;
    let _ = queue.enqueue(task(Priority::Low, "good", None)).await?;

    let leased = queue.lease(10, Duration::from_secs(60)).await?;
    assert_eq!(leased.len(), 1);
    assert_eq!(leased[0].task.payload, "good");
    let depth = queue.depth().await?;
    assert_eq!(depth.dead_lettered, 1, "{depth:?}");
    Ok(())
}
