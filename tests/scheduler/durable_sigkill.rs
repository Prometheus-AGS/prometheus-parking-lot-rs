//! Scenario S-16: a pool process killed with `SIGKILL` loses no accepted task.
//!
//! Each test starts a child process (this test binary, running
//! [`durable_child_entry`]) that opens the durable backend, starts a
//! `DurableResourcePool`, submits one task and prints `ACCEPTED` once `submit`
//! has returned. The child then hangs at the kill point and announces it:
//!
//! - `EXECUTING`: inside the executor (mid-execution);
//! - `DELIVERING`: after the executor returned, inside the mailbox write (the
//!   mailbox wrapper never forwards the delivery).
//!
//! The parent kills the child there, then plays the restarted process: it
//! opens the same backend, starts a fresh pool and requires that the task runs
//! again once the dead process's lease expires, that its result arrives, that
//! exactly one result is consumable, and that the queue is empty (acked).

use std::future::Future;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use prometheus_parking_lot::core::{
    Delivery, DeliveryOutcome, LeaseQueue, MailboxEntry, ResultMailbox, SchedulerError, TaskStatus,
};
use prometheus_parking_lot::util::MailboxKey;
use prometheus_parking_lot::{DurableOptions, DurableResourcePool};

use crate::support::{Behaviour, Executor, announce, eventually, key, limits, spawner, task};

const ENV_BACKEND: &str = "PLQ_S16_BACKEND";
const ENV_TARGET: &str = "PLQ_S16_TARGET";
const ENV_KILL_AT: &str = "PLQ_S16_KILL_AT";
/// This test's name as libtest lists it, for `--exact`.
const CHILD_TEST: &str = "durable_sigkill::durable_child_entry";
/// Queue and mailbox name on Postgres.
#[cfg(feature = "postgres")]
const PG_NAME: &str = "s16";
/// Short, so the restarted pool sees the dead process's lease expire quickly.
const LEASE: Duration = Duration::from_secs(2);
/// The mailbox session the task delivers to.
const SESSION: &str = "job-1";
/// How long the parent waits for the child to reach the kill point.
const CHILD_TIMEOUT: Duration = Duration::from_secs(120);

fn options() -> DurableOptions {
    DurableOptions::new(LEASE)
        .with_name("s16")
        .with_poll_interval(Duration::from_millis(100))
}

/// Where the child is killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KillAt {
    Execution,
    Delivery,
}

impl KillAt {
    const fn env(self) -> &'static str {
        match self {
            Self::Execution => "execute",
            Self::Delivery => "deliver",
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Execution => "EXECUTING",
            Self::Delivery => "DELIVERING",
        }
    }
}

/// A mailbox whose `deliver` announces `DELIVERING` and never stores: the
/// process is killed between the execution and the mailbox write.
struct HangOnDeliver<M>(M);

impl<M: ResultMailbox<String>> ResultMailbox<String> for HangOnDeliver<M> {
    async fn deliver(&self, delivery: Delivery<String>) -> Result<DeliveryOutcome, SchedulerError> {
        drop(delivery);
        announce("DELIVERING");
        std::future::pending().await
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.0.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.0.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.0.evict_expired()
    }
}

// ── the child process ───────────────────────────────────────────────────────

/// Entry point of the pool child process; a no-op in a normal test run.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_child_entry() {
    let (Ok(backend), Ok(target), Ok(kill_at)) = (
        std::env::var(ENV_BACKEND),
        std::env::var(ENV_TARGET),
        std::env::var(ENV_KILL_AT),
    ) else {
        return;
    };
    let kill_at = if kill_at == KillAt::Delivery.env() {
        KillAt::Delivery
    } else {
        KillAt::Execution
    };
    match backend.as_str() {
        #[cfg(feature = "postgres")]
        "pg" => {
            use prometheus_parking_lot::infra::mailbox::PgResultMailbox;
            use prometheus_parking_lot::infra::queue::PgLeaseQueue;
            let pool = crate::support::pg::connect(&target).await;
            let queue = PgLeaseQueue::<String>::new(pool.clone(), PG_NAME, 100);
            let mailbox = PgResultMailbox::<String>::new(pool, PG_NAME);
            run_child(queue, mailbox, kill_at).await;
        }
        #[cfg(feature = "embedded")]
        "redb" => {
            let store = prometheus_parking_lot::infra::EmbeddedStore::open(&target)
                .expect("child: open the store");
            run_child(
                store.lease_queue::<String>(100),
                store.result_mailbox(),
                kill_at,
            )
            .await;
        }
        other => panic!("child: unknown backend {other}"),
    }
}

/// Start the pool, accept one task and wait to be killed at `kill_at`.
async fn run_child<Q, M>(queue: Q, mailbox: M, kill_at: KillAt)
where
    Q: LeaseQueue<String> + 'static,
    M: ResultMailbox<String> + 'static,
{
    match kill_at {
        KillAt::Execution => {
            let executor = Executor::new(Behaviour::HangAfterAnnouncing);
            let pool = DurableResourcePool::start(
                limits(4),
                queue,
                mailbox,
                executor,
                spawner(),
                options(),
            )
            .expect("child: start the pool");
            accept_and_wait(&pool).await;
        }
        KillAt::Delivery => {
            let executor = Executor::new(Behaviour::Succeed);
            let pool = DurableResourcePool::start(
                limits(4),
                queue,
                HangOnDeliver(mailbox),
                executor,
                spawner(),
                options(),
            )
            .expect("child: start the pool");
            accept_and_wait(&pool).await;
        }
    }
}

async fn accept_and_wait<Q, M>(
    pool: &DurableResourcePool<
        String,
        String,
        Q,
        M,
        Executor,
        prometheus_parking_lot::runtime::TokioSpawner,
    >,
) where
    Q: LeaseQueue<String> + 'static,
    M: ResultMailbox<String> + 'static,
{
    let status = pool
        .submit(task(SESSION, "work-1"))
        .await
        .expect("child: submit");
    assert_eq!(status, TaskStatus::Queued);
    announce("ACCEPTED");
    std::future::pending::<()>().await;
}

// ── the parent ──────────────────────────────────────────────────────────────

/// Run the child and `SIGKILL` it once it announces `kill_at`'s marker, which
/// it must do after it announced `ACCEPTED`.
fn run_and_kill(backend: &str, target: &str, kill_at: KillAt) {
    let mut child = Command::new(std::env::current_exe().expect("current exe"))
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(ENV_BACKEND, backend)
        .env(ENV_TARGET, target)
        .env(ENV_KILL_AT, kill_at.env())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("spawn the pool process");
    let watchdog = watchdog(&child);
    let stdout = child.stdout.take().expect("child stdout");

    let mut accepted = false;
    let mut killed = false;
    for line in BufReader::new(stdout).lines() {
        let line = line.expect("read child stdout");
        // With `--nocapture` libtest prints "test <name> ... " without a
        // newline before the test's own output, so markers are matched at the
        // end of the line.
        if line.ends_with("ACCEPTED") {
            accepted = true;
        } else if line.ends_with(kill_at.marker()) {
            assert!(
                accepted,
                "the task reached {kill_at:?} before submit returned"
            );
            // SIGKILL: no destructor, flush or unwinding runs in the child.
            child.kill().expect("SIGKILL the pool process");
            killed = true;
            break;
        }
    }
    drop(watchdog);
    let status = child.wait().expect("wait for the pool process");
    assert!(
        killed,
        "the pool process exited ({status}) before reaching {kill_at:?}"
    );
    assert!(!status.success(), "the pool process must have been killed");
}

/// Kill the child with `kill -9` if it has not reached its kill point in
/// [`CHILD_TIMEOUT`]; dropping the returned sender disarms it.
fn watchdog(child: &Child) -> std::sync::mpsc::Sender<()> {
    let pid = child.id().to_string();
    let (disarm, disarmed) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || {
        if disarmed.recv_timeout(CHILD_TIMEOUT) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
            eprintln!("watchdog: the pool process did not reach its kill point; killing it");
            let _ = Command::new("kill").args(["-9", &pid]).status();
        }
    });
    disarm
}

/// The restarted process: a fresh pool on the same backend must re-run the
/// task after the dead process's lease expires and leave exactly one result.
async fn restart_and_check<Q, M>(queue: Q, mailbox: M)
where
    Q: LeaseQueue<String> + 'static,
    M: ResultMailbox<String> + 'static,
{
    let executor = Executor::new(Behaviour::Succeed);
    let pool = DurableResourcePool::start(
        limits(4),
        queue,
        mailbox,
        executor.clone(),
        spawner(),
        options(),
    )
    .expect("start the restarted pool");
    let key = key(SESSION);
    let (pool_ref, key_ref) = (&pool, &key);

    let entry = eventually(LEASE + Duration::from_secs(30), || async move {
        pool_ref.mailbox().consume(key_ref).await.expect("consume")
    })
    .await
    .expect("no result after restart: the accepted task was lost");
    assert_eq!(entry.status, TaskStatus::Completed);
    assert_eq!(entry.payload.as_deref(), Some("done:work-1"));

    // The lease is resolved after the delivery: the queue drains.
    let drained = eventually(Duration::from_secs(10), || async move {
        let depth = pool_ref.queue().depth().await.expect("depth");
        (depth.live() == 0).then_some(depth)
    })
    .await;
    assert!(drained.is_some(), "the task was never acked");

    // Exactly one result: nothing else arrives, even after another lease.
    tokio::time::sleep(LEASE + Duration::from_secs(1)).await;
    assert!(
        pool.mailbox()
            .consume(&key)
            .await
            .expect("consume")
            .is_none(),
        "a second result was consumable"
    );
    assert_eq!(executor.runs(), 1, "the restarted pool runs the task once");
    let depth = pool.queue().depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 0), "{depth:?}");
}

#[cfg(feature = "postgres")]
mod postgres {
    use prometheus_parking_lot::infra::mailbox::PgResultMailbox;
    use prometheus_parking_lot::infra::queue::PgLeaseQueue;

    use super::{KillAt, PG_NAME, restart_and_check, run_and_kill};
    use crate::support::pg::{Pg18, connect};

    async fn killed_and_restarted(kill_at: KillAt) {
        let pg = Pg18::start().await;
        let pool = connect(&pg.url).await;
        PgLeaseQueue::<String>::migrate(&pool)
            .await
            .expect("migrate queue");
        PgResultMailbox::<String>::migrate(&pool)
            .await
            .expect("migrate mailbox");

        let url = pg.url.clone();
        tokio::task::spawn_blocking(move || run_and_kill("pg", &url, kill_at))
            .await
            .expect("pool process");

        restart_and_check(
            PgLeaseQueue::<String>::new(pool.clone(), PG_NAME, 100),
            PgResultMailbox::<String>::new(pool, PG_NAME),
        )
        .await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg:18 container; kills a child process"]
    async fn pool_sigkill_pg_mid_execution() {
        killed_and_restarted(KillAt::Execution).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg:18 container; kills a child process"]
    async fn pool_sigkill_pg_between_execution_and_delivery() {
        killed_and_restarted(KillAt::Delivery).await;
    }
}

#[cfg(feature = "embedded")]
mod redb {
    use prometheus_parking_lot::infra::EmbeddedStore;

    use super::{KillAt, restart_and_check, run_and_kill};

    async fn killed_and_restarted(kill_at: KillAt) {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("s16.redb");
        let path = file.to_str().expect("utf-8 path").to_owned();
        tokio::task::spawn_blocking(move || run_and_kill("redb", &path, kill_at))
            .await
            .expect("pool process");

        // The killed process held the file; its lock went with it.
        let store = EmbeddedStore::open(&file).expect("reopen the store");
        restart_and_check(store.lease_queue::<String>(100), store.result_mailbox()).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_sigkill_redb_mid_execution() {
        killed_and_restarted(KillAt::Execution).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_sigkill_redb_between_execution_and_delivery() {
        killed_and_restarted(KillAt::Delivery).await;
    }
}
