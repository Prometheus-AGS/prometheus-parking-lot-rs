//! Real-Postgres tests for [`PgmqLeaseQueue`] (cr-30, scenario S-18) on the
//! project Postgres + pgmq image.
//!
//! Every test needs Docker, so each is `#[ignore]`d and compiles in CI without
//! it. Run them with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-pg --locked --run-ignored all -E 'test(pgmq_)'
//! ```
//!
//! `PARKING_LOT_PG_MAJOR=17` runs them against the PG 17 image instead of 18.
//!
//! - `pgmq_conformance::*`: the shared backend conformance suite (ADR 0003);
//! - `pgmq_priority_order`, `pgmq_unacked_task_reappears_after_visibility_timeout`,
//!   `pgmq_stale_token_is_rejected`: the S-18 oracle;
//! - `pgmq_sigkill_redelivery`: a consumer killed with `SIGKILL` mid-task, the
//!   task redelivered after the visibility timeout, the dead token fenced off;
//! - `pgmq_concurrent_dispatchers`: eight dispatchers never double-claim;
//! - idempotency, max attempts, dead letters, wake-up and name validation.
//!
//! The negative control (`RUSTFLAGS='--cfg parking_lot_negative_control="s-18"'`)
//! collapses the priority lanes into one pgmq queue; the priority-order cases
//! must then fail.

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
use prometheus_parking_lot_pg::{PgmqLeaseQueue, Wake};
use sqlx::postgres::PgPoolOptions;
use support::{PgContainer, TestResult, pg_major};

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

/// A container with pgmq present and the shared schema applied.
async fn start() -> TestResult<PgContainer> {
    let pg = PgContainer::start(pg_major()).await?;
    PgmqLeaseQueue::<String>::migrate(&pg.pool).await?;
    Ok(pg)
}

/// A prepared queue named `name` on `pg`.
async fn queue(
    pg: &PgContainer,
    name: &str,
    max_depth: usize,
) -> TestResult<PgmqLeaseQueue<String>> {
    let queue = PgmqLeaseQueue::new(pg.pool.clone(), name, max_depth)?;
    queue.prepare().await?;
    Ok(queue)
}

// ── conformance ─────────────────────────────────────────────────────────────

/// Builds a fresh queue name per queue; time moves by really sleeping, because
/// visibility timeouts run on the database clock.
struct PgmqQueueHarness {
    pg: PgContainer,
    next: AtomicU32,
}

impl PgmqQueueHarness {
    async fn start() -> Self {
        Self {
            pg: start().await.expect("start Postgres and migrate"),
            next: AtomicU32::new(0),
        }
    }
}

impl QueueHarness for PgmqQueueHarness {
    type Queue = PgmqLeaseQueue<String>;

    fn queue(&self, max_depth: usize) -> impl Future<Output = Self::Queue> + Send {
        let name = format!("conf{}", self.next.fetch_add(1, Ordering::Relaxed));
        let pool = self.pg.pool.clone();
        async move {
            let queue = PgmqLeaseQueue::new(pool, name, max_depth).expect("valid queue name");
            queue.prepare().await.expect("prepare pgmq queues");
            queue
        }
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(by)
    }

    fn lease_duration(&self) -> Duration {
        Duration::from_secs(3)
    }
}

mod pgmq_conformance {
    use super::*;

    lease_queue_conformance!(
        test_attr = #[cfg_attr(all(), tokio::test, ignore = "needs Docker: starts a parking-lot-pg container")],
        harness = PgmqQueueHarness::start().await,
    );
}

// ── S-18 oracle ─────────────────────────────────────────────────────────────

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_version_is_recorded_and_supported() -> TestResult<()> {
    let pg = start().await?;
    let version: String =
        sqlx::query_scalar("SELECT extversion FROM pg_extension WHERE extname = 'pgmq'")
            .fetch_one(&pg.pool)
            .await?;
    let server: String = sqlx::query_scalar("SHOW server_version")
        .fetch_one(&pg.pool)
        .await?;
    println!("pgmq {version} on PostgreSQL {server}");
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0));
    assert!(
        (major, minor) >= (1, 8),
        "pgmq >= 1.8 required, found {version}"
    );
    Ok(())
}

/// Tasks of three priorities, enqueued lowest first: they are leased highest
/// first, one lease call at a time, and in enqueue order within a priority.
#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_priority_order() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "prio", 100).await?;
    for (priority, payload) in [
        (Priority::Low, "low-1"),
        (Priority::Low, "low-2"),
        (Priority::Normal, "normal-1"),
        (Priority::High, "high-1"),
        (Priority::High, "high-2"),
    ] {
        let _ = queue.enqueue(task(priority, payload, None)).await?;
    }
    let mut order = Vec::new();
    for _ in 0..5 {
        let leased = queue.lease(1, Duration::from_secs(30)).await?;
        assert_eq!(leased.len(), 1);
        order.push(leased[0].task.payload.clone());
    }
    assert_eq!(order, ["high-1", "high-2", "normal-1", "low-1", "low-2"]);
    Ok(())
}

/// An unacked task is invisible while its visibility timeout runs and
/// reappears after it, as attempt 2, without any reaper.
#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_unacked_task_reappears_after_visibility_timeout() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "vt", 100).await?;
    let _ = queue.enqueue(task(Priority::Normal, "t", None)).await?;
    let first = queue.lease(1, Duration::from_secs(2)).await?.remove(0);

    assert!(queue.lease(1, Duration::from_secs(2)).await?.is_empty());
    tokio::time::sleep(Duration::from_millis(2600)).await;
    let second = queue.lease(1, Duration::from_secs(2)).await?.remove(0);
    assert_eq!(second.attempt, 2);
    assert_eq!(second.token.entry(), first.token.entry());

    // The visibility timeout is the lease: pgmq itself recorded the redelivery.
    let read_ct: i32 = sqlx::query_scalar(
        "SELECT max(read_ct) FROM (SELECT read_ct FROM pgmq.q_pl_vt_p0
              UNION ALL SELECT read_ct FROM pgmq.q_pl_vt_p1
              UNION ALL SELECT read_ct FROM pgmq.q_pl_vt_p2
              UNION ALL SELECT read_ct FROM pgmq.q_pl_vt_p3) lanes",
    )
    .fetch_one(&pg.pool)
    .await?;
    assert_eq!(read_ct, 2);
    Ok(())
}

/// A token from an earlier delivery is rejected by ack, nack and extend, and
/// the rejected ack does not delete the message the new holder owns.
#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_stale_token_is_rejected() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "stale", 100).await?;
    let _ = queue.enqueue(task(Priority::High, "t", None)).await?;
    let first = queue.lease(1, Duration::from_secs(1)).await?.remove(0);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let second = queue.lease(1, Duration::from_secs(60)).await?.remove(0);
    assert_ne!(first.token, second.token);

    for result in [
        queue.ack(first.token).await,
        queue.nack(first.token, NackAction::requeue()).await,
        queue
            .nack(first.token, NackAction::dead_letter("stale"))
            .await,
        queue.extend(first.token, Duration::from_secs(60)).await,
    ] {
        assert!(
            matches!(result, Err(SchedulerError::LeaseLost { entry }) if entry == first.token.entry().0),
            "{result:?}"
        );
    }
    // The current holder is untouched: still leased, can still ack.
    let depth = queue.depth().await?;
    assert_eq!((depth.leased, depth.dead_lettered), (1, 0), "{depth:?}");
    queue.ack(second.token).await?;
    assert_eq!(queue.depth().await?.live(), 0);

    // A token that no pgmq message can match is stale too, not an error.
    let alien = LeaseToken::new(QueueEntryId(u64::MAX), u64::MAX);
    assert!(matches!(
        queue.ack(alien).await,
        Err(SchedulerError::LeaseLost { .. })
    ));
    Ok(())
}

// ── SIGKILL redelivery ──────────────────────────────────────────────────────

const CHILD_URL: &str = "PLQ_PGMQ_CHILD_URL";
const CHILD_QUEUE: &str = "PLQ_PGMQ_CHILD_QUEUE";
const CHILD_LEASE: Duration = Duration::from_secs(4);

/// Entry point of the consumer child process; a no-op in a normal test run.
///
/// It leases one task, reports the lease on stdout and then "works" forever;
/// the parent kills it with `SIGKILL` while it holds the lease.
#[tokio::test]
async fn pgmq_sigkill_child_entry() {
    let (Ok(url), Ok(name)) = (std::env::var(CHILD_URL), std::env::var(CHILD_QUEUE)) else {
        return;
    };
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("child: connect");
    let queue = PgmqLeaseQueue::<String>::new(pool, name, 100).expect("child: queue");
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
            "pgmq_sigkill_child_entry",
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
async fn pgmq_sigkill_redelivery() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "sigkill", 100).await?;
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

    // The consumer is dead but its visibility timeout still holds.
    assert!(
        queue.lease(10, CHILD_LEASE).await?.is_empty(),
        "a task under a live lease was handed out"
    );
    let depth = queue.depth().await?;
    assert_eq!((depth.ready, depth.leased), (0, 1), "{depth:?}");

    // After it expires the task is redelivered as a new attempt.
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

    // The dead consumer's token is fenced off, however late it shows up.
    assert!(matches!(
        queue.ack(killed.token).await,
        Err(SchedulerError::LeaseLost { entry: e }) if e == entry.0
    ));

    // Completed exactly once.
    queue.ack(redelivered.token).await?;
    assert!(matches!(
        queue.ack(redelivered.token).await,
        Err(SchedulerError::LeaseLost { .. })
    ));
    assert_eq!(queue.depth().await?.live(), 0);
    assert!(queue.lease(10, Duration::from_secs(1)).await?.is_empty());
    Ok(())
}

// ── no double claim ─────────────────────────────────────────────────────────

const DISPATCHERS: usize = 8;
const TASKS: usize = 4_000;
const BATCH: usize = 25;

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_concurrent_dispatchers() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "dispatch", TASKS).await?;

    // Load all four lanes in bulk through `pgmq.send_batch`; one task through
    // the real enqueue path proves the stored shape is the one `lease` reads.
    let first = queue
        .enqueue(task(Priority::Normal, "task-0", None))
        .await?;
    assert!(matches!(first, Enqueued::Accepted(_)));
    let per_lane = (TASKS - 1) / 4;
    let mut loaded = 1;
    for (lane, priority) in [
        Priority::Low,
        Priority::Normal,
        Priority::High,
        Priority::Critical,
    ]
    .into_iter()
    .enumerate()
    {
        let count = if lane == 3 {
            TASKS - 1 - 3 * per_lane
        } else {
            per_lane
        };
        let meta = serde_json::to_value(task(priority, "", None).meta)?;
        sqlx::query(
            "SELECT pgmq.send_batch($1::text,
                    ARRAY(SELECT jsonb_build_object('meta', $2::jsonb,
                                                    'payload', 'task-' || $3 || '-' || g)
                            FROM generate_series(1, $4) g))",
        )
        .bind(format!("pl_dispatch_p{lane}"))
        .bind(meta)
        .bind(i32::try_from(lane)?)
        .bind(i32::try_from(count)?)
        .execute(&pg.pool)
        .await?;
        loaded += count;
    }
    assert_eq!(loaded, TASKS);
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

// ── idempotency, attempts, dead letters, wake-up, names ─────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_idempotency_first_write_wins_under_contention() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "idem", 100).await?;
    let mut attempts = Vec::new();
    for i in 0..16 {
        let queue = queue.clone();
        attempts.push(tokio::spawn(async move {
            queue
                .enqueue(task(
                    Priority::Normal,
                    &format!("writer-{i}"),
                    Some("same-key"),
                ))
                .await
        }));
    }
    let mut accepted = Vec::new();
    let mut duplicates = Vec::new();
    for attempt in attempts {
        match attempt.await?? {
            Enqueued::Accepted(id) => accepted.push(id),
            Enqueued::Duplicate(id) => duplicates.push(id),
        }
    }
    assert_eq!(accepted.len(), 1, "exactly one writer wins");
    assert_eq!(duplicates.len(), 15);
    assert!(duplicates.iter().all(|id| *id == accepted[0]));
    assert_eq!(queue.depth().await?.live(), 1);

    // The key is released with the entry.
    let leased = queue.lease(1, Duration::from_secs(30)).await?.remove(0);
    queue.ack(leased.token).await?;
    let aux: i64 = sqlx::query_scalar("SELECT count(*) FROM pl_pgmq_aux")
        .fetch_one(&pg.pool)
        .await?;
    assert_eq!(aux, 0, "ack removes the side-table row");
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_max_attempts_and_dead_letter_reason() -> TestResult<()> {
    let pg = start().await?;
    let queue =
        PgmqLeaseQueue::<String>::new(pg.pool.clone(), "attempts", 100)?.with_max_attempts(2);
    queue.prepare().await?;
    let _ = queue
        .enqueue(task(Priority::Normal, "poison", Some("poison-key")))
        .await?;
    for expected in 1..=2 {
        let leased = queue.lease(1, Duration::from_secs(1)).await?;
        assert_eq!(leased[0].attempt, expected);
        tokio::time::sleep(Duration::from_millis(1500)).await;
    }
    // Third delivery would exceed the limit: dead-lettered instead.
    assert!(queue.lease(1, Duration::from_secs(1)).await?.is_empty());
    let depth = queue.depth().await?;
    assert_eq!((depth.live(), depth.dead_lettered), (0, 1), "{depth:?}");

    let reason: String =
        sqlx::query_scalar("SELECT message->>'reason' FROM pgmq.q_pl_attempts_dead")
            .fetch_one(&pg.pool)
            .await?;
    assert_eq!(reason, "max attempts (2) exceeded");
    // Dead-lettering released the idempotency key.
    assert!(matches!(
        queue
            .enqueue(task(Priority::Normal, "poison", Some("poison-key")))
            .await?,
        Enqueued::Accepted(_)
    ));

    // An explicit dead letter carries its reason too.
    let leased = queue.lease(1, Duration::from_secs(30)).await?.remove(0);
    queue
        .nack(leased.token, NackAction::dead_letter("operator said no"))
        .await?;
    let reasons: Vec<String> = sqlx::query_scalar(
        "SELECT message->>'reason' FROM pgmq.q_pl_attempts_dead ORDER BY msg_id",
    )
    .fetch_all(&pg.pool)
    .await?;
    assert_eq!(reasons, ["max attempts (2) exceeded", "operator said no"]);
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_undecodable_message_is_dead_lettered() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "poisoned", 100).await?;
    sqlx::query("SELECT pgmq.send('pl_poisoned_p1', '{\"unexpected\": true}'::jsonb)")
        .execute(&pg.pool)
        .await?;
    let _ = queue.enqueue(task(Priority::Normal, "good", None)).await?;

    let leased = queue.lease(10, Duration::from_secs(30)).await?;
    assert_eq!(
        leased.len(),
        1,
        "the undecodable message must not wedge the lease"
    );
    assert_eq!(leased[0].task.payload, "good");
    assert_eq!(queue.depth().await?.dead_lettered, 1);
    Ok(())
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_notify_wakes_an_idle_dispatcher() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "wake", 100).await?;
    let other = self::queue(&pg, "wake_other", 100).await?;
    let mut listener = queue.listener().await?;

    assert_eq!(
        listener.wait(Duration::from_millis(300)).await?,
        Wake::TimedOut
    );
    let _ = other
        .enqueue(task(Priority::Normal, "elsewhere", None))
        .await?;
    assert_eq!(
        listener.wait(Duration::from_millis(500)).await?,
        Wake::TimedOut
    );

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

    // An immediate requeue wakes; a backed-off one does not.
    let leased = queue.lease(1, Duration::from_secs(60)).await?.remove(0);
    queue.nack(leased.token, NackAction::requeue()).await?;
    assert_eq!(listener.wait(Duration::from_secs(5)).await?, Wake::Notified);
    let leased = queue.lease(1, Duration::from_secs(60)).await?.remove(0);
    queue
        .nack(
            leased.token,
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
async fn pgmq_queue_names_are_validated() {
    let pool = sqlx::PgPool::connect_lazy("postgres://nobody@localhost/none").expect("lazy pool");
    for bad in [
        "",
        "Upper",
        "has space",
        "semi;colon",
        "dash-ed",
        &"x".repeat(39),
    ] {
        assert!(
            matches!(
                PgmqLeaseQueue::<String>::new(pool.clone(), bad, 1),
                Err(SchedulerError::InvalidConfig { .. })
            ),
            "{bad:?} must be rejected"
        );
    }
    assert!(PgmqLeaseQueue::<String>::new(pool, "ok_name_1", 1).is_ok());
}

#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_failed_dead_letter_does_not_discard_leased_tasks() -> TestResult<()> {
    let pg = start().await?;
    let queue = PgmqLeaseQueue::<String>::new(pg.pool.clone(), "dlfail", 100)?.with_max_attempts(1);
    queue.prepare().await?;
    let _ = queue.enqueue(task(Priority::Normal, "spent", None)).await?;
    assert_eq!(queue.lease(1, Duration::from_secs(1)).await?.len(), 1);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let _ = queue.enqueue(task(Priority::Normal, "good", None)).await?;
    // The dead-letter queue is gone, so dead-lettering "spent" must fail.
    sqlx::query("SELECT pgmq.drop_queue('pl_dlfail_dead')")
        .execute(&pg.pool)
        .await?;

    let leased = queue.lease(10, Duration::from_secs(30)).await?;
    assert_eq!(
        leased.len(),
        1,
        "the good task was leased and must be returned"
    );
    assert_eq!(leased[0].task.payload, "good");
    Ok(())
}

// ── fence vs. in-flight statements (S2) ─────────────────────────────────────

/// Number of backends (other than this one) waiting on a lock.
async fn lock_waiters(pool: &sqlx::PgPool) -> TestResult<i64> {
    Ok(sqlx::query_scalar(
        "SELECT count(*) FROM pg_stat_activity
          WHERE wait_event_type = 'Lock' AND pid <> pg_backend_pid()",
    )
    .fetch_one(pool)
    .await?)
}

async fn wait_for_lock_waiters(pool: &sqlx::PgPool, want: i64) -> TestResult<()> {
    let deadline = Instant::now() + Duration::from_secs(20);
    while lock_waiters(pool).await? < want {
        assert!(
            Instant::now() < deadline,
            "{want} lock waiter(s) never appeared"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

/// A heartbeat `extend` already waiting on the row lock behind a delayed nack
/// must not re-arm the nacked token. Dropping a Rust future does not cancel its
/// SQL statement, so such an `extend` can outlive the call that issued it; once
/// the nack commits it re-checks from its old snapshot, passes, and would set
/// `vt = now + lease`: after the delay the nacked token would hold the message
/// again. Two connections and an ordered lock queue make the interleaving
/// deterministic: the lock holder releases only after the nack and then the
/// extend are both parked on the row.
#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_extend_in_flight_cannot_rearm_a_nacked_token() -> TestResult<()> {
    use sqlx::Connection;

    let pg = start().await?;
    let queue = queue(&pg, "fence", 100).await?;
    let _ = queue.enqueue(task(Priority::Normal, "t", None)).await?;
    let leased = queue.lease(1, Duration::from_secs(60)).await?.remove(0);
    let token = leased.token;
    let msg_id = i64::try_from(token.entry().0)? / 4;

    // Hold the message row so both statements park on it, in a known order.
    let mut holder = sqlx::PgConnection::connect(&pg.url).await?;
    sqlx::query("BEGIN").execute(&mut holder).await?;
    sqlx::query("SELECT 1 FROM pgmq.q_pl_fence_p1 WHERE msg_id = $1 FOR UPDATE")
        .bind(msg_id)
        .execute(&mut holder)
        .await?;

    let nack = tokio::spawn({
        let queue = queue.clone();
        async move {
            queue
                .nack(token, NackAction::requeue_after(Duration::from_secs(2)))
                .await
        }
    });
    wait_for_lock_waiters(&pg.pool, 1).await?;
    let extend = tokio::spawn({
        let queue = queue.clone();
        async move { queue.extend(token, Duration::from_secs(60)).await }
    });
    wait_for_lock_waiters(&pg.pool, 2).await?;
    sqlx::query("ROLLBACK").execute(&mut holder).await?;

    nack.await??;
    let extended = extend.await?;
    assert!(
        matches!(extended, Err(SchedulerError::LeaseLost { .. })),
        "an extend queued behind a nack must lose the lease: {extended:?}"
    );

    // After the delay the token must still be dead and the message deliverable.
    tokio::time::sleep(Duration::from_millis(2500)).await;
    let acked = queue.ack(token).await;
    assert!(
        matches!(acked, Err(SchedulerError::LeaseLost { .. })),
        "the nacked token held the message again: {acked:?}"
    );
    let again = queue.lease(1, Duration::from_secs(30)).await?;
    assert_eq!(again.len(), 1, "the nacked message is redelivered");
    assert_eq!(again[0].attempt, 2);
    Ok(())
}

/// A read error on a lower lane must not discard what higher lanes already
/// leased in the same call: those messages are invisible until their leases
/// run out, so dropping them would stall them for a whole lease. With nothing
/// leased yet the error still surfaces.
#[tokio::test]
#[ignore = "needs Docker: starts a parking-lot-pg container"]
async fn pgmq_lease_read_error_keeps_what_higher_lanes_leased() -> TestResult<()> {
    let pg = start().await?;
    let queue = queue(&pg, "partial", 100).await?;
    let _ = queue
        .enqueue(task(Priority::Critical, "urgent", None))
        .await?;
    // Lane 0 is read last; make its read fail.
    sqlx::query("ALTER TABLE pgmq.q_pl_partial_p0 RENAME TO q_pl_partial_p0_broken")
        .execute(&pg.pool)
        .await?;

    let leased = queue.lease(5, Duration::from_secs(30)).await?;
    assert_eq!(
        leased.len(),
        1,
        "the critical task was leased and must be returned"
    );
    assert_eq!(leased[0].task.payload, "urgent");

    // Nothing left in the higher lanes: the lane-0 failure is reported.
    let result = queue.lease(5, Duration::from_secs(30)).await;
    assert!(
        matches!(result, Err(SchedulerError::Backend { .. })),
        "{result:?}"
    );
    Ok(())
}
