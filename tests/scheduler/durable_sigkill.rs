//! Scenario S-16: a pool process killed with `SIGKILL` loses no accepted task.
//!
//! Each test starts a child process (this test binary, running
//! [`durable_child_entry`]) that opens the durable backend, starts a
//! `DurableResourcePool`, submits one task and prints `ACCEPTED` once `submit`
//! has returned. The child then hangs at the kill point and announces it:
//!
//! - `EXECUTING`: inside the executor (mid-execution);
//! - `DELIVERING`: after the executor returned, inside the mailbox write (the
//!   mailbox wrapper never forwards the delivery);
//! - `DELIVERED`: after the result was stored in the mailbox and before the
//!   lease was acked (the wrapper forwards the delivery, then hangs).
//!
//! The parent kills the child there, then plays the restarted process: it
//! opens the same backend, starts a fresh pool **under a different name** and
//! requires that the task runs again once the dead process's lease expires,
//! that its result arrives, that exactly one result is consumable (after the
//! `DELIVERED` kill the restarted run's delivery must be recognised as a
//! duplicate although the pools are named differently), and that the queue is
//! empty (acked).
//!
//! OA-07 adds a Failed `DELIVERED` kill point. The renamed restarted pool
//! locally succeeds, but must emit the original failure and dead-letter the
//! lease. Both retained and consumed-before-retry results use the same real
//! child/SIGKILL protocol. A separate real-store fixture seeds a nonterminal
//! fence and requires hand-back instead of a fabricated terminal outcome.

use std::future::Future;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use prometheus_parking_lot::core::{
    Delivery, DeliveryOutcome, LeaseQueue, MailboxEntry, ResultMailbox, SchedulerError,
    ShutdownPolicy, TaskError, TaskStatus,
};
use prometheus_parking_lot::util::MailboxKey;
use prometheus_parking_lot::{DurableOptions, DurableResourcePool, LifecycleEvent};

use crate::support::{
    Behaviour, Executor, announce, event_log, eventually, key, limits, spawner, task,
};

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
/// A payload-independent child error, so `work-1` succeeds after restart.
const ORIGINAL_FAILURE: &str = "OA-07 original executor failure before SIGKILL";
const FAILED_CALLER_KEY: &str = "oa-07-failed-before-resolve";

fn options() -> DurableOptions {
    DurableOptions::new(LEASE)
        .with_name("s16")
        .with_poll_interval(Duration::from_millis(100))
}

/// The restarted process's pool name: the result key must not depend on it.
const RESTARTED_NAME: &str = "s16-restarted";

/// Where the child is killed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KillAt {
    Execution,
    Delivery,
    AfterDelivery,
    FailedAfterDelivery,
}

impl KillAt {
    const fn env(self) -> &'static str {
        match self {
            Self::Execution => "execute",
            Self::Delivery => "deliver",
            Self::AfterDelivery => "delivered",
            Self::FailedAfterDelivery => "failed-delivered",
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Execution => "EXECUTING",
            Self::Delivery => "DELIVERING",
            Self::AfterDelivery | Self::FailedAfterDelivery => "DELIVERED",
        }
    }

    fn from_env(value: &str) -> Self {
        [
            Self::Execution,
            Self::Delivery,
            Self::AfterDelivery,
            Self::FailedAfterDelivery,
        ]
        .into_iter()
        .find(|kill_at| kill_at.env() == value)
        .unwrap_or_else(|| panic!("child: unknown kill point {value}"))
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

/// A mailbox that stores the delivery, announces `DELIVERED` and never
/// returns: the process is killed after the result is stored and before the
/// lease is acked.
struct HangAfterDeliver<M>(M);

impl<M: ResultMailbox<String>> ResultMailbox<String> for HangAfterDeliver<M> {
    async fn deliver(&self, delivery: Delivery<String>) -> Result<DeliveryOutcome, SchedulerError> {
        let _stored = self.0.deliver(delivery).await?;
        announce("DELIVERED");
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
    let kill_at = KillAt::from_env(&kill_at);
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
            accept_and_wait(&pool, false).await;
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
            accept_and_wait(&pool, false).await;
        }
        KillAt::AfterDelivery | KillAt::FailedAfterDelivery => {
            let failed = kill_at == KillAt::FailedAfterDelivery;
            let executor = Executor::new(if failed {
                Behaviour::Fail(ORIGINAL_FAILURE)
            } else {
                Behaviour::Succeed
            });
            let pool = DurableResourcePool::start(
                limits(4),
                queue,
                HangAfterDeliver(mailbox),
                executor,
                spawner(),
                options(),
            )
            .expect("child: start the pool");
            accept_and_wait(&pool, failed).await;
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
    failed: bool,
) where
    Q: LeaseQueue<String> + 'static,
    M: ResultMailbox<String> + 'static,
{
    let mut submitted = task(SESSION, "work-1");
    if failed {
        submitted.meta.idempotency_key = Some(FAILED_CALLER_KEY.to_owned());
    }
    let status = pool.submit(submitted).await.expect("child: submit");
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
        options().with_name(RESTARTED_NAME),
    )
    .expect("start the restarted pool");
    let key = key(SESSION);
    let (pool_ref, key_ref) = (&pool, &key);

    let entry = eventually(LEASE + Duration::from_secs(120), || async move {
        pool_ref.mailbox().consume(key_ref).await.expect("consume")
    })
    .await
    .expect("no result after restart: the accepted task was lost");
    assert_eq!(entry.status, TaskStatus::Completed);
    assert_eq!(entry.payload.as_deref(), Some("done:work-1"));

    // The lease is resolved after the delivery: the queue drains.
    let drained = eventually(Duration::from_secs(60), || async move {
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

/// Inspect the original child failure and optionally consume its sole result
/// before the renamed pool retries the task.
async fn original_failed_entry<M: ResultMailbox<String>>(
    mailbox: &M,
    address: &MailboxKey,
    error: &TaskError,
    consume_before_retry: bool,
) -> MailboxEntry<String> {
    let original = mailbox
        .peek(address)
        .await
        .expect("peek")
        .expect("stored child failure");
    assert_eq!(
        original.status,
        TaskStatus::Failed(error.clone()),
        "OA-07:stored-status"
    );
    assert_eq!(original.task_id, 1);
    assert_eq!(original.idempotency_key.as_deref(), Some(FAILED_CALLER_KEY));
    assert_eq!(
        original.payload, None,
        "the child's failure has no success payload"
    );
    if consume_before_retry {
        assert_eq!(
            mailbox
                .consume(address)
                .await
                .expect("consume before retry"),
            Some(original.clone()),
            "the sole original failure is consumed before the pool restarts"
        );
        assert!(
            mailbox
                .consume(address)
                .await
                .expect("consume again")
                .is_none()
        );
    }

    original
}

/// The retried lease stays dead-lettered and cannot create another result,
/// including after the original child result was already consumed.
async fn assert_failed_result_stays_unique<Q, M>(
    queue: &Q,
    mailbox: &M,
    executor: &Executor,
    address: &MailboxKey,
    original: MailboxEntry<String>,
    consume_before_retry: bool,
) where
    Q: LeaseQueue<String>,
    M: ResultMailbox<String>,
{
    let after_retry = mailbox.consume(address).await.expect("consume after retry");
    if consume_before_retry {
        assert_eq!(
            after_retry, None,
            "OA-07:one-result: retry must not replace the consumed failure"
        );
    } else {
        assert_eq!(
            after_retry,
            Some(original),
            "OA-07:one-result: preserve the original failure"
        );
    }
    tokio::time::sleep(LEASE + Duration::from_secs(1)).await;
    assert!(
        mailbox
            .consume(address)
            .await
            .expect("consume after another lease")
            .is_none(),
        "OA-07:one-result: no second consumable result"
    );
    let depth = queue.depth().await.expect("depth after another lease");
    assert_eq!(
        (depth.live(), depth.dead_lettered),
        (0, 1),
        "OA-07:lease-resolution: {depth:?}"
    );
    assert_eq!(executor.runs(), 1);
}

/// OA-07: the child stored a failure but died before resolving its lease.
/// The same task retries successfully under a different pool name. Await any
/// terminal event so bool-only handling fails at the event oracle immediately,
/// rather than timing out while waiting specifically for Failed.
async fn restart_failed_and_check<Q, M>(queue: Q, mailbox: M, consume_before_retry: bool)
where
    Q: LeaseQueue<String> + 'static,
    M: ResultMailbox<String> + 'static,
{
    let address = key(SESSION);
    let error = TaskError::executor(ORIGINAL_FAILURE);
    let original = original_failed_entry(&mailbox, &address, &error, consume_before_retry).await;

    let executor = Executor::new(Behaviour::Succeed);
    let (bus, mut log) = event_log();
    let pool = DurableResourcePool::start(
        limits(4),
        queue,
        mailbox,
        executor.clone(),
        spawner(),
        options().with_name(RESTARTED_NAME).with_hooks(bus.clone()),
    )
    .expect("start the restarted pool");
    let terminal = log
        .wait_for(|event| event.task_id() == 1 && event.is_terminal())
        .await;
    assert!(
        matches!(&terminal, Some(LifecycleEvent::Failed { pool, error: stored, .. })
            if pool == RESTARTED_NAME && stored == &error.to_string()),
        "OA-07:terminal-event: retry must report original Failed, got {terminal:?}"
    );

    let pool_ref = &pool;
    let drained = eventually(Duration::from_secs(60), || async move {
        let depth = pool_ref.queue().depth().await.expect("depth");
        (depth.live() == 0).then_some(depth)
    })
    .await
    .expect("OA-07:lease-resolution: the retried lease must resolve");
    assert_eq!(
        (drained.live(), drained.dead_lettered),
        (0, 1),
        "OA-07:lease-resolution: original Failed must dead-letter, never ack: {drained:?}"
    );
    let report = pool
        .shutdown_with(ShutdownPolicy::Drain {
            deadline: Duration::from_secs(30),
        })
        .await;
    assert_eq!(
        report.unsettled, 0,
        "failure settlement drained: {report:?}"
    );
    assert!(bus.flush(Duration::from_secs(5)), "flush restarted hooks");
    assert_eq!(bus.dropped_events(), 0);
    log.drain_ready();
    assert_eq!(
        log.seen
            .iter()
            .filter(|event| event.task_id() == 1 && event.is_terminal())
            .count(),
        1,
        "OA-07:terminal-event: exactly one restarted terminal event: {:?}",
        log.kinds_of(1)
    );
    assert!(
        !log.seen
            .iter()
            .any(|event| matches!(event, LifecycleEvent::Completed { .. }))
    );
    assert_eq!(
        executor.runs(),
        1,
        "the renamed retry really executed work-1 successfully once"
    );

    assert_failed_result_stays_unique(
        pool.queue(),
        pool.mailbox(),
        &executor,
        &address,
        original,
        consume_before_retry,
    )
    .await;
}

/// A legal generic mailbox may remember a current, nonterminal status.
/// Exercise the real pool/backend boundary; the local successful retry cannot
/// turn that record into terminal truth or destroy the accepted queue entry.
async fn nonterminal_duplicate_and_check<Q, M>(queue: Q, mailbox: M, status: TaskStatus)
where
    Q: LeaseQueue<String> + 'static,
    M: ResultMailbox<String> + 'static,
{
    let address = key("oa-07-nonterminal");
    let caller_key = "oa-07-nonterminal-fence";
    assert_eq!(
        mailbox
            .deliver(
                Delivery::<String>::new(address.clone(), 1, status.clone(), None)
                    .with_idempotency_key(caller_key)
            )
            .await
            .expect("seed current status"),
        DeliveryOutcome::Stored
    );
    let mut pending = task("oa-07-nonterminal", "work-1");
    pending.meta.idempotency_key = Some(caller_key.to_owned());
    let _enqueued = queue.enqueue(pending).await.expect("enqueue the real task");
    let executor = Executor::new(Behaviour::Succeed);
    let (bus, mut log) = event_log();
    let pool = DurableResourcePool::start(
        limits(4),
        queue,
        mailbox,
        executor.clone(),
        spawner(),
        options().with_name(RESTARTED_NAME).with_hooks(bus.clone()),
    )
    .expect("start the nonterminal-fence consumer");
    let terminal = log.wait_for(LifecycleEvent::is_terminal).await;
    // Stop further leasing once the first run ending is observable; more than
    // one immediate hand-back may already have raced the asynchronous hook.
    pool.shutdown();
    assert!(
        matches!(terminal, Some(LifecycleEvent::Requeued { task_id: 1, .. })),
        "OA-07:nonterminal-event: stored {status:?} must hand back, got {terminal:?}"
    );
    let report = pool
        .shutdown_with(ShutdownPolicy::Drain {
            deadline: Duration::from_secs(30),
        })
        .await;
    assert_eq!(
        report.unsettled, 0,
        "nonterminal hand-back settled: {report:?}"
    );
    assert!(bus.flush(Duration::from_secs(5)), "flush hand-back hooks");
    assert_eq!(bus.dropped_events(), 0);
    log.drain_ready();
    assert!(
        log.seen
            .iter()
            .filter(|event| event.is_terminal())
            .all(|event| matches!(event, LifecycleEvent::Requeued { .. })),
        "OA-07:nonterminal-event: no fabricated terminal outcome: {:?}",
        log.kinds_of(1)
    );
    assert!(
        executor.runs() >= 1,
        "the real executor retried the queued task"
    );
    let depth = pool.queue().depth().await.expect("nonterminal queue depth");
    assert_eq!(
        (depth.live(), depth.dead_lettered),
        (1, 0),
        "OA-07:nonterminal-resolution: nonterminal metadata cannot ack or dead-letter: {depth:?}"
    );
    assert_eq!(
        (depth.ready, depth.delayed, depth.leased),
        (1, 0, 0),
        "OA-07:nonterminal-resolution: the entry is handed back ready for another consumer: {depth:?}"
    );
    let entry = pool
        .mailbox()
        .consume(&address)
        .await
        .expect("consume original current status")
        .expect("original current status is retained");
    assert_eq!(entry.status, status);
    assert_eq!(entry.payload, None);
    assert!(
        pool.mailbox()
            .consume(&address)
            .await
            .expect("consume again")
            .is_none()
    );
}

#[cfg(feature = "postgres")]
mod postgres {
    use prometheus_parking_lot::infra::mailbox::PgResultMailbox;
    use prometheus_parking_lot::infra::queue::PgLeaseQueue;

    use super::{KillAt, PG_NAME, restart_and_check, restart_failed_and_check, run_and_kill};
    use crate::support::pg::{Pg18, connect};

    async fn killed_and_restarted(kill_at: KillAt, consume_before_retry: bool) {
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

        let queue = PgLeaseQueue::<String>::new(pool.clone(), PG_NAME, 100);
        let mailbox = PgResultMailbox::<String>::new(pool, PG_NAME);
        if kill_at == KillAt::FailedAfterDelivery {
            restart_failed_and_check(queue, mailbox, consume_before_retry).await;
        } else {
            restart_and_check(queue, mailbox).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg:18 container; kills a child process"]
    async fn pool_sigkill_pg_mid_execution() {
        killed_and_restarted(KillAt::Execution, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg:18 container; kills a child process"]
    async fn pool_sigkill_pg_between_execution_and_delivery() {
        killed_and_restarted(KillAt::Delivery, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: starts a parking-lot-pg:18 container; kills a child process"]
    async fn pool_sigkill_pg_between_delivery_and_ack() {
        killed_and_restarted(KillAt::AfterDelivery, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: PARKING_LOT_PG_MAJOR=17|18; kills a child after Failed delivery"]
    async fn pool_sigkill_pg_failed_delivery_retained_after_restart() {
        killed_and_restarted(KillAt::FailedAfterDelivery, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "needs Docker: PARKING_LOT_PG_MAJOR=17|18; consumes Failed before the retry"]
    async fn pool_sigkill_pg_failed_delivery_consumed_before_restart() {
        killed_and_restarted(KillAt::FailedAfterDelivery, true).await;
    }
}

#[cfg(feature = "embedded")]
mod redb {
    use prometheus_parking_lot::infra::EmbeddedStore;

    use super::{
        KillAt, nonterminal_duplicate_and_check, restart_and_check, restart_failed_and_check,
        run_and_kill,
    };
    use prometheus_parking_lot::core::TaskStatus;

    async fn killed_and_restarted(kill_at: KillAt, consume_before_retry: bool) {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("s16.redb");
        let path = file.to_str().expect("utf-8 path").to_owned();
        tokio::task::spawn_blocking(move || run_and_kill("redb", &path, kill_at))
            .await
            .expect("pool process");

        // The killed process held the file; its lock went with it.
        let store = EmbeddedStore::open(&file).expect("reopen the store");
        if kill_at == KillAt::FailedAfterDelivery {
            restart_failed_and_check(
                store.lease_queue::<String>(100),
                store.result_mailbox(),
                consume_before_retry,
            )
            .await;
        } else {
            restart_and_check(store.lease_queue::<String>(100), store.result_mailbox()).await;
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_sigkill_redb_mid_execution() {
        killed_and_restarted(KillAt::Execution, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_sigkill_redb_between_execution_and_delivery() {
        killed_and_restarted(KillAt::Delivery, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_sigkill_redb_between_delivery_and_ack() {
        killed_and_restarted(KillAt::AfterDelivery, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_sigkill_redb_failed_delivery_retained_after_restart() {
        killed_and_restarted(KillAt::FailedAfterDelivery, false).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_sigkill_redb_failed_delivery_consumed_before_restart() {
        killed_and_restarted(KillAt::FailedAfterDelivery, true).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn pool_duplicate_redb_nonterminal_status_requeues_without_false_terminal() {
        for status in [TaskStatus::Queued, TaskStatus::Running] {
            let dir = tempfile::tempdir().expect("tempdir");
            let store = EmbeddedStore::open(dir.path().join("oa07-nonterminal.redb"))
                .expect("open real nonterminal-fence store");
            nonterminal_duplicate_and_check(
                store.lease_queue::<String>(100),
                store.result_mailbox(),
                status,
            )
            .await;
        }
    }
}
