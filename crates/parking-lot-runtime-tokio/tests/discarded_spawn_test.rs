//! OA-08: a legal spawner may drop an accepted run without polling it.
//!
//! These fixtures enter through `ResourcePool::submit`, the production
//! dispatcher, mailbox and hook bus, then `shutdown_with`. The negative control
//! `--cfg parking_lot_negative_control="oa-08"` moves ownership back inside an
//! unpolled async body: the discarded-run outcome and shutdown oracles fail,
//! while the ordinary `TokioSpawner` control still completes normally. A child
//! process also covers an inline-discarding spawner draining a large parked
//! backlog on a fixed-size stack; stack overflow must not abort the suite.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

use prometheus_parking_lot_core::{
    Hook, HookBus, InMemoryMailbox, InMemoryQueue, LifecycleEvent, Mailbox, MailboxKey, PoolLimits,
    Priority, RecordingHook, ResourceCost, ResourceKind, ScheduledTask, SchedulerError, Spawn,
    TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot_runtime_tokio::{
    ResourcePool, ShutdownPolicy, ShutdownReport, TokioSpawner,
};

const WAIT: Duration = Duration::from_secs(5);
const DRAIN: Duration = Duration::from_secs(60);
const DISCARDED: &str = "spawner discarded resource pool task before its first poll";

#[derive(Clone, Copy, Serialize, Deserialize)]
enum Job {
    Echo,
    Panic,
    Pending,
}

#[derive(Clone)]
struct Executor(Arc<Mutex<Vec<u64>>>);

#[async_trait]
impl TaskExecutor<Job, u64> for Executor {
    async fn execute(&self, job: Job, meta: TaskMetadata) -> Result<u64, TaskError> {
        self.0.lock().push(meta.id);
        match job {
            Job::Echo => Ok(meta.id),
            Job::Panic => panic!("oa-08 executor panic"),
            Job::Pending => std::future::pending().await,
        }
    }
}

struct DiscardSpawner;

impl Spawn for DiscardSpawner {
    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        drop(future);
    }
}

type Unpolled = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Holds futures unpolled so shutdown is already waiting when they drop.
#[derive(Clone, Default)]
struct HoldingSpawner(Arc<Mutex<Vec<Unpolled>>>);

impl Spawn for HoldingSpawner {
    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.0.lock().push(Box::pin(future));
    }
}

impl HoldingSpawner {
    fn discard_all(&self) {
        loop {
            let held = std::mem::take(&mut *self.0.lock());
            if held.is_empty() {
                return;
            }
            // Drop outside the spawner lock: completing a discarded run may
            // launch parked work through this same spawner.
            drop(held);
        }
    }
}

/// Holds the first run while the backlog is parked, then drops every
/// successor inline. Unlike `HoldingSpawner` this deliberately nests `Spawn`
/// calls on the completing thread, reproducing the recursive-drop defect.
#[derive(Clone, Default)]
struct InlineAfterReleaseSpawner(Arc<Mutex<InlineAfterRelease>>);

#[derive(Default)]
struct InlineAfterRelease {
    first: Option<Unpolled>,
    released: bool,
}

impl Spawn for InlineAfterReleaseSpawner {
    fn spawn<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let mut state = self.0.lock();
        if !state.released {
            assert!(
                state.first.is_none(),
                "only one unit can start before release"
            );
            state.first = Some(Box::pin(future));
            return;
        }
        drop(state);
        drop(future);
    }
}

impl InlineAfterReleaseSpawner {
    fn release(&self) {
        let first = {
            let mut state = self.0.lock();
            state.released = true;
            state.first.take().expect("first run was held")
        };
        drop(first);
    }
}

#[derive(Clone, Default)]
struct SharedMailbox {
    stored: Arc<Mutex<InMemoryMailbox<u64>>>,
    deliveries: Arc<Mutex<Vec<MailboxKey>>>,
    panic_after_store: bool,
}

impl Mailbox<u64> for SharedMailbox {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<u64>,
    ) -> Result<(), SchedulerError> {
        self.stored.lock().deliver(key, status, payload)?;
        self.deliveries.lock().push(key.clone());
        assert!(!self.panic_after_store, "oa-08 mailbox callback panic");
        Ok(())
    }
}

impl SharedMailbox {
    fn take_once(&self, id: u64) -> (TaskStatus, Option<u64>) {
        let key = key(id);
        assert_eq!(
            self.deliveries.lock().iter().filter(|k| **k == key).count(),
            1,
            "one delivery per task"
        );
        let mut stored = self.stored.lock();
        let outcome = stored
            .consume(&key)
            .expect("consume")
            .expect("task outcome");
        assert!(stored.consume(&key).expect("consume again").is_none());
        drop(stored);
        outcome
    }
}

fn key(id: u64) -> MailboxKey {
    MailboxKey {
        tenant: "oa-08".into(),
        user_id: None,
        session_id: Some(id.to_string()),
    }
}

fn task(id: u64, payload: Job, mailbox: bool) -> ScheduledTask<Job> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: mailbox.then(|| key(id)),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
        },
        payload,
    }
}

fn pool<S: Spawn + Send + Sync + 'static>(
    spawner: S,
    mailbox: &SharedMailbox,
    executions: &Arc<Mutex<Vec<u64>>>,
    hook: &Arc<RecordingHook>,
) -> ResourcePool<Job, u64, InMemoryQueue<Job>, SharedMailbox, Executor, S> {
    ResourcePool::new(
        PoolLimits {
            max_units: 1,
            max_queue_depth: 8,
            default_timeout: Duration::from_millis(50),
        },
        InMemoryQueue::new(8),
        mailbox.clone(),
        Executor(Arc::clone(executions)),
        spawner,
    )
    .with_hook(Arc::clone(hook) as Arc<dyn Hook>)
}

async fn events(bus: &HookBus, hook: &RecordingHook) -> Vec<LifecycleEvent> {
    let bus = bus.clone();
    assert!(
        tokio::task::spawn_blocking(move || bus.flush(WAIT))
            .await
            .expect("flush thread")
    );
    hook.drain()
}

fn assert_discarded(events: &[LifecycleEvent], id: u64) {
    let task_events: Vec<_> = events
        .iter()
        .filter(|event| event.task_id() == id)
        .collect();
    assert!(matches!(
        task_events.first(),
        Some(LifecycleEvent::Submitted { .. })
    ));
    assert!(
        !task_events
            .iter()
            .any(|event| matches!(event, LifecycleEvent::Started { .. }))
    );
    let terminal: Vec<_> = task_events
        .iter()
        .filter(|event| event.is_terminal())
        .collect();
    assert_eq!(terminal.len(), 1, "{events:?}");
    assert!(
        matches!(terminal[0], LifecycleEvent::Failed { error, .. } if error.contains(DISCARDED))
    );
}

fn assert_cancelled(outcome: (TaskStatus, Option<u64>)) {
    assert!(matches!(
        outcome,
        (TaskStatus::Failed(TaskError::Cancelled { reason }), None) if reason == DISCARDED
    ));
}

#[tokio::test]
async fn discarded_spawn_reports_once_and_returns_capacity_without_polling() {
    let mailbox = SharedMailbox::default();
    let executions = Arc::new(Mutex::new(Vec::new()));
    let hook = Arc::new(RecordingHook::new(32));
    let pool = pool(DiscardSpawner, &mailbox, &executions, &hook);
    for id in 0..3 {
        assert_eq!(
            pool.submit(task(id, Job::Echo, true))
                .await
                .expect("accepted"),
            TaskStatus::Running
        );
        assert_eq!(pool.running_tasks(), 0, "discarded run returns its unit");
        assert_eq!(pool.active_units(), 0);
        assert_eq!(pool.queued_tasks(), 0);
    }
    assert_eq!(
        pool.failed_tasks(),
        3,
        "OA-08:discarded-terminal: each discarded run reports failure"
    );
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("discarded runs must not hold shutdown until the drain deadline");
    assert_eq!(
        report,
        ShutdownReport::default(),
        "settled before shutdown began"
    );
    assert_eq!((pool.completed_tasks(), pool.failed_tasks()), (0, 3));
    assert!(
        executions.lock().is_empty(),
        "no discarded payload was executed"
    );
    let events = events(pool.hooks(), &hook).await;
    for id in 0..3 {
        assert_cancelled(mailbox.take_once(id));
        assert_discarded(&events, id);
    }
}

#[tokio::test]
async fn discarded_spawn_wakes_shutdown_and_settles_parked_work() {
    let mailbox = SharedMailbox::default();
    let executions = Arc::new(Mutex::new(Vec::new()));
    let hook = Arc::new(RecordingHook::new(32));
    let spawner = HoldingSpawner::default();
    let pool = pool(spawner.clone(), &mailbox, &executions, &hook);
    for id in 0..3 {
        pool.submit(task(id, Job::Echo, true))
            .await
            .expect("accepted");
    }
    assert_eq!((pool.running_tasks(), pool.queued_tasks()), (1, 2));
    let shutdown = pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN });
    tokio::pin!(shutdown);
    tokio::select! {
        biased;
        report = &mut shutdown => panic!("shutdown returned before futures were discarded: {report:?}"),
        () = tokio::task::yield_now() => {},
    }
    spawner.discard_all();
    let report = tokio::time::timeout(WAIT, shutdown)
        .await
        .expect("settlement must wake shutdown before its drain deadline");
    assert_eq!(
        (report.completed, report.cancelled, report.unsettled),
        (3, 0, 0)
    );
    assert_eq!((pool.running_tasks(), pool.queued_tasks()), (0, 0));
    assert_eq!(pool.active_units(), 0);
    assert_eq!(pool.failed_tasks(), 3);
    assert!(executions.lock().is_empty());
    let events = events(pool.hooks(), &hook).await;
    for id in 0..3 {
        assert_cancelled(mailbox.take_once(id));
        assert_discarded(&events, id);
    }
}

#[tokio::test]
async fn discarded_spawn_without_mailbox_still_reports_and_settles() {
    let mailbox = SharedMailbox::default();
    let executions = Arc::new(Mutex::new(Vec::new()));
    let hook = Arc::new(RecordingHook::new(8));
    let pool = pool(DiscardSpawner, &mailbox, &executions, &hook);
    pool.submit(task(0, Job::Echo, false))
        .await
        .expect("accepted");
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("no mailbox is needed to settle a discarded run");
    assert_eq!(report.unsettled, 0);
    assert_eq!((pool.running_tasks(), pool.failed_tasks()), (0, 1));
    assert!(mailbox.deliveries.lock().is_empty());
    assert!(executions.lock().is_empty());
    assert_discarded(&events(pool.hooks(), &hook).await, 0);
}

#[tokio::test]
async fn discarded_spawn_contains_mailbox_callback_panic_and_settles_once() {
    let mailbox = SharedMailbox {
        panic_after_store: true,
        ..SharedMailbox::default()
    };
    let executions = Arc::new(Mutex::new(Vec::new()));
    let hook = Arc::new(RecordingHook::new(32));
    let spawner = InlineAfterReleaseSpawner::default();
    let pool = pool(spawner.clone(), &mailbox, &executions, &hook);
    for id in 0..3 {
        pool.submit(task(id, Job::Echo, true))
            .await
            .expect("accepted");
    }
    // Each mailbox stores then panics. The drainer must contain the panic
    // and continue with successors already enqueued by inline Spawn drops.
    spawner.release();
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("callback panic cannot leave the ledger unsettled");
    assert_eq!(report.unsettled, 0);
    assert_eq!((pool.running_tasks(), pool.failed_tasks()), (0, 3));
    assert_eq!((pool.queued_tasks(), pool.active_units()), (0, 0));
    let events = events(pool.hooks(), &hook).await;
    for id in 0..3 {
        assert_cancelled(mailbox.take_once(id));
        assert_discarded(&events, id);
    }
}

#[tokio::test]
async fn discarded_spawn_ordinary_spawner_preserves_success_panic_and_timeout_once() {
    let mailbox = SharedMailbox::default();
    let executions = Arc::new(Mutex::new(Vec::new()));
    let hook = Arc::new(RecordingHook::new(32));
    let pool = pool(
        TokioSpawner::new(tokio::runtime::Handle::current()),
        &mailbox,
        &executions,
        &hook,
    );
    for (id, job) in [(0, Job::Echo), (1, Job::Panic), (2, Job::Pending)] {
        pool.submit(task(id, job, true)).await.expect("accepted");
    }
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("ordinary runs settle under their budgets");
    assert_eq!(report.unsettled, 0);
    assert_eq!((pool.running_tasks(), pool.queued_tasks()), (0, 0));
    assert_eq!((pool.completed_tasks(), pool.failed_tasks()), (1, 2));
    assert_eq!(*executions.lock(), vec![0, 1, 2]);
    assert_eq!(mailbox.take_once(0), (TaskStatus::Completed, Some(0)));
    assert!(matches!(
        mailbox.take_once(1),
        (TaskStatus::Failed(TaskError::ExecutorPanicked { .. }), None)
    ));
    assert!(matches!(
        mailbox.take_once(2),
        (TaskStatus::Failed(TaskError::Timeout { .. }), None)
    ));
    let events = events(pool.hooks(), &hook).await;
    for id in 0..3 {
        assert_eq!(
            events
                .iter()
                .filter(|event| event.task_id() == id && event.is_terminal())
                .count(),
            1
        );
    }
}

/// The historical bug aborts its process on stack overflow rather than
/// unwinding. Keep that failure isolated and require a normal child exit plus
/// the marker written only after every public-path oracle has passed.
#[test]
fn discarded_spawn_large_backlog_is_stack_bounded() {
    use std::process::{Command, Stdio};
    use std::time::Instant;

    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "discarded_spawn_large_backlog_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("PARKING_LOT_OA08_BACKLOG_CHILD", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start discarded-backlog child");
    let deadline = Instant::now() + Duration::from_secs(90);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill stalled child");
            let output = child.wait_with_output().expect("reap stalled child");
            panic!(
                "OA-08:discarded-backlog-watchdog: no completion evidence: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = child.wait_with_output().expect("collect child output");
    assert!(
        status.success(),
        "OA-08:discarded-backlog: child did not settle the inline-discard backlog: {status}\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("OA-08:discarded-backlog-settled"),
        "OA-08:discarded-backlog: child success requires the settlement marker"
    );
}

#[test]
fn discarded_spawn_large_backlog_child() {
    if !matches!(
        std::env::var("PARKING_LOT_OA08_BACKLOG_CHILD").as_deref(),
        Ok("1")
    ) {
        return;
    }
    std::thread::Builder::new()
        .name("oa08-bounded-stack".into())
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .expect("child runtime")
                .block_on(async {
                    const TASKS: usize = 16_384;
                    let mailbox = SharedMailbox::default();
                    let executions = Arc::new(Mutex::new(Vec::new()));
                    let spawner = InlineAfterReleaseSpawner::default();
                    // No hooks in the stress leg: the bounded hook channel may
                    // legally drop events under this burst. Existing fixtures
                    // separately assert each discarded-run lifecycle event.
                    let pool = ResourcePool::new(
                        PoolLimits {
                            max_units: 1,
                            max_queue_depth: TASKS,
                            default_timeout: Duration::from_secs(1),
                        },
                        InMemoryQueue::new(TASKS),
                        mailbox.clone(),
                        Executor(Arc::clone(&executions)),
                        spawner.clone(),
                    );
                    for id in 0..TASKS as u64 {
                        pool.submit(task(id, Job::Echo, true))
                            .await
                            .expect("accepted");
                    }
                    assert_eq!((pool.running_tasks(), pool.queued_tasks()), (1, TASKS - 1));
                    let shutdown = pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN });
                    tokio::pin!(shutdown);
                    tokio::select! {
                        biased;
                        report = &mut shutdown => panic!("shutdown preceded release: {report:?}"),
                        () = tokio::task::yield_now() => {},
                    }
                    // The old implementation recursively settles every parked
                    // successor here and overflows the fixed 2 MiB stack.
                    spawner.release();
                    assert_eq!(
                        pool.failed_tasks(),
                        TASKS as u64,
                        "OA-08:discarded-backlog: every accepted task failed exactly once"
                    );
                    assert_eq!((pool.running_tasks(), pool.queued_tasks()), (0, 0));
                    assert_eq!(pool.active_units(), 0, "every permit returned");
                    assert_eq!(pool.completed_tasks(), 0);
                    assert!(executions.lock().is_empty(), "no payload executed");
                    let report = tokio::time::timeout(WAIT, shutdown)
                        .await
                        .expect("settlement wakes shutdown before its deadline");
                    assert_eq!(
                        (report.completed, report.cancelled, report.unsettled),
                        (TASKS as u64, 0, 0),
                        "one settled ledger outcome per task"
                    );
                    let deliveries = mailbox.deliveries.lock();
                    assert_eq!(deliveries.len(), TASKS, "one delivery per accepted task");
                    let mut counts = vec![0_usize; TASKS];
                    for delivered in deliveries.iter() {
                        let id: usize = delivered
                            .session_id
                            .as_ref()
                            .expect("task key")
                            .parse()
                            .expect("numeric task key");
                        counts[id] += 1;
                    }
                    assert!(
                        counts.iter().all(|count| *count == 1),
                        "no duplicate or missing delivery"
                    );
                    drop(deliveries);
                    let mut stored = mailbox.stored.lock();
                    for id in 0..TASKS as u64 {
                        let key = key(id);
                        assert_cancelled(
                            stored.consume(&key).expect("consume").expect("task result"),
                        );
                        assert!(stored.consume(&key).expect("consume again").is_none());
                    }
                    drop(stored);
                    println!("OA-08:discarded-backlog-settled");
                });
        })
        .expect("bounded-stack thread")
        .join()
        .expect("backlog assertions");
}
