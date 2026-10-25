//! OA-10: explicit pruning reports each expired task through the real pool.
//!
//! `parking_lot_negative_control="oa-10-expiry"` omits `ResourcePool` Expired
//! events while retaining removal, mailbox delivery and settlement. The main
//! fixture must fail its exact sequence oracle; the live-deadline control passes.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;

use prometheus_parking_lot_core::{
    AuditEvent, AuditSink, Hook, HookBus, InMemoryAuditSink, InMemoryMailbox, InMemoryQueue,
    LifecycleEvent, Mailbox, MailboxKey, PoolLimits, Priority, ResourceCost, ResourceKind,
    ScheduledTask, SchedulerError, TaskError, TaskExecutor, TaskMetadata, TaskStatus, TestClock,
};
use prometheus_parking_lot_runtime_tokio::{ResourcePool, ShutdownPolicy, TokioSpawner};

const WAIT: Duration = Duration::from_secs(5);
const DRAIN: Duration = Duration::from_secs(60);

/// No Clone bound is needed even on the production pruning path.
#[derive(Serialize, Deserialize)]
struct Job {
    id: u64,
    block: bool,
}

#[derive(Clone)]
struct Executor {
    calls: Arc<Mutex<Vec<u64>>>,
    gate: Arc<Semaphore>,
    started: Arc<Semaphore>,
}

impl Executor {
    fn new() -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            gate: Arc::new(Semaphore::new(0)),
            started: Arc::new(Semaphore::new(0)),
        }
    }
}

#[async_trait]
impl TaskExecutor<Job, u64> for Executor {
    async fn execute(&self, job: Job, meta: TaskMetadata) -> Result<u64, TaskError> {
        assert_eq!(job.id, meta.id);
        self.calls.lock().push(job.id);
        self.started.add_permits(1);
        if job.block {
            self.gate
                .acquire()
                .await
                .expect("gate remains open")
                .forget();
        }
        Ok(job.id)
    }
}

#[derive(Clone, Default)]
struct SharedMailbox {
    stored: Arc<Mutex<InMemoryMailbox<u64>>>,
    deliveries: Arc<Mutex<Vec<MailboxKey>>>,
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
        Ok(())
    }
}

impl SharedMailbox {
    fn take(&self, id: u64) -> Option<(TaskStatus, Option<u64>)> {
        self.stored.lock().consume(&key(id)).expect("consume")
    }

    fn assert_delivered_once(&self, id: u64) {
        assert_eq!(
            self.deliveries
                .lock()
                .iter()
                .filter(|entry| **entry == key(id))
                .count(),
            1
        );
    }
}

#[derive(Default)]
struct Log(Mutex<Vec<LifecycleEvent>>);

impl Hook for Log {
    fn on_event(&self, event: &LifecycleEvent) {
        self.0.lock().push(event.clone());
    }
}

#[derive(Clone)]
struct Audit(Arc<Mutex<InMemoryAuditSink>>);

impl AuditSink for Audit {
    fn record(&mut self, event: AuditEvent) {
        self.0.lock().record(event);
    }
}

fn key(id: u64) -> MailboxKey {
    MailboxKey {
        tenant: format!("oa-10-tenant-{id}"),
        user_id: Some(format!("user-{id}")),
        session_id: Some(format!("session-{id}")),
    }
}

fn task(id: u64, deadline_ms: Option<u128>, priority: Priority) -> ScheduledTask<Job> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(key(id)),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms,
            created_at_ms: 7,
            idempotency_key: Some(format!("original-{id}")),
            sequence: 0,
        },
        payload: Job { id, block: id == 0 },
    }
}

type Pool = ResourcePool<Job, u64, InMemoryQueue<Job>, SharedMailbox, Executor, TokioSpawner>;

fn pool(
    executor: &Executor,
    mailbox: &SharedMailbox,
    clock: &TestClock,
    log: &Arc<Log>,
    audit: &Audit,
) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units: 1,
            max_queue_depth: 8,
            default_timeout: DRAIN,
        },
        InMemoryQueue::new(8),
        mailbox.clone(),
        executor.clone(),
        TokioSpawner::new(tokio::runtime::Handle::current()),
    )
    .with_clock(clock.clone())
    .with_max_head_skips(0)
    .with_hook(Arc::clone(log) as Arc<dyn Hook>)
    .with_audit(Box::new(audit.clone()))
}

async fn flush(bus: &HookBus, log: &Log) -> Vec<LifecycleEvent> {
    let copied = bus.clone();
    assert!(
        tokio::task::spawn_blocking(move || copied.flush(WAIT))
            .await
            .expect("flush thread")
    );
    assert_eq!(bus.dropped_events(), 0);
    log.0.lock().clone()
}

fn kinds(events: &[LifecycleEvent], id: u64) -> Vec<&'static str> {
    events
        .iter()
        .filter(|event| event.task_id() == id)
        .map(LifecycleEvent::kind)
        .collect()
}

async fn start_blocker(pool: &Pool, executor: &Executor) {
    pool.submit(task(0, None, Priority::Normal))
        .await
        .expect("blocker");
    tokio::time::timeout(WAIT, executor.started.acquire())
        .await
        .expect("blocker starts")
        .expect("started gate")
        .forget();
    assert_eq!(*executor.calls.lock(), vec![0]);
}

/// Every removed task keeps its own expiry audit identity.
fn assert_expiry_audit(audit: &Audit) {
    let audit_events = audit.0.lock().events();
    let expired: Vec<_> = audit_events
        .iter()
        .filter(|event| event.action == "expire")
        .collect();
    assert_eq!(
        expired.len(),
        2,
        "one task-specific expiry audit per removal"
    );
    for id in [1, 3] {
        assert!(
            expired
                .iter()
                .any(|event| event.task_id == id.to_string() && event.tenant == key(id).tenant)
        );
    }
    assert!(!audit_events.iter().any(|event| event.task_id == "batch"));
}

#[tokio::test]
async fn prune_expiry_reports_each_front_and_backend_task_once_without_execution() {
    let executor = Executor::new();
    let mailbox = SharedMailbox::default();
    let clock = TestClock::new(99);
    let log = Arc::new(Log::default());
    let audit = Audit(Arc::new(Mutex::new(InMemoryAuditSink::new(64))));
    let pool = pool(&executor, &mailbox, &clock, &log, &audit);
    start_blocker(&pool, &executor).await;
    // Task 1 enters the adapter front. The live high-priority head (2)
    // precedes it; task 3 stays expired behind live work in the backend.
    for item in [
        task(1, Some(100), Priority::Normal),
        task(2, None, Priority::High),
        task(3, Some(100), Priority::Low),
        task(4, None, Priority::Normal),
    ] {
        assert_eq!(pool.submit(item).await.expect("parked"), TaskStatus::Queued);
    }
    clock.set_wall_ms(100);
    assert_eq!(pool.prune_expired().await.expect("live at equality"), 0);
    assert_eq!(pool.queued_tasks(), 4);
    clock.set_wall_ms(101);
    assert_eq!(pool.prune_expired().await.expect("explicit prune"), 2);
    assert_eq!(pool.prune_expired().await.expect("repeat prune"), 0);
    assert_eq!(
        (
            pool.active_units(),
            pool.queued_tasks(),
            pool.failed_tasks()
        ),
        (1, 2, 2)
    );
    assert_eq!(
        *executor.calls.lock(),
        vec![0],
        "pruned payloads never execute"
    );
    for id in [1, 3] {
        assert_eq!(mailbox.take(id), Some((TaskStatus::Expired, None)));
        assert!(mailbox.take(id).is_none());
        mailbox.assert_delivered_once(id);
    }
    executor.gate.add_permits(1);
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("survivors drain with no unsettled or double-settled work");
    assert_eq!(
        (report.completed, report.cancelled, report.unsettled),
        (3, 0, 0)
    );
    assert_eq!(
        (
            pool.completed_tasks(),
            pool.failed_tasks(),
            pool.active_units(),
            pool.queued_tasks()
        ),
        (3, 2, 0, 0)
    );
    assert_eq!(
        *executor.calls.lock(),
        vec![0, 2, 4],
        "live survivor priority/order"
    );
    for id in [0, 2, 4] {
        assert_eq!(mailbox.take(id), Some((TaskStatus::Completed, Some(id))));
        mailbox.assert_delivered_once(id);
    }
    for id in [1, 3] {
        assert!(mailbox.take(id).is_none());
        mailbox.assert_delivered_once(id);
    }
    assert_expiry_audit(&audit);
    // All accounting/mailbox/survivor checks precede this exact sequence
    // oracle: the oa-10-expiry control fails here, not on a timeout.
    let events = flush(pool.hooks(), &log).await;
    for id in [1, 3] {
        assert_eq!(
            kinds(&events, id),
            vec!["submitted", "queued", "expired"],
            "task {id}"
        );
    }
    for id in [0, 2, 4] {
        assert_eq!(
            events
                .iter()
                .filter(|event| event.task_id() == id && event.is_terminal())
                .count(),
            1
        );
    }
}

#[tokio::test]
async fn prune_expiry_live_deadline_control_still_completes() {
    let executor = Executor::new();
    let mailbox = SharedMailbox::default();
    let clock = TestClock::new(99);
    let log = Arc::new(Log::default());
    let audit = Audit(Arc::new(Mutex::new(InMemoryAuditSink::new(32))));
    let pool = pool(&executor, &mailbox, &clock, &log, &audit);
    start_blocker(&pool, &executor).await;
    pool.submit(task(1, Some(100), Priority::Normal))
        .await
        .expect("live follower");
    clock.set_wall_ms(100);
    assert_eq!(pool.prune_expired().await.expect("deadline equality"), 0);
    assert_eq!(pool.prune_expired().await.expect("repeat equality"), 0);
    assert_eq!(pool.failed_tasks(), 0);
    executor.gate.add_permits(1);
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("live work finishes");
    assert_eq!(
        (report.completed, report.cancelled, report.unsettled),
        (2, 0, 0)
    );
    assert_eq!(*executor.calls.lock(), vec![0, 1]);
    assert_eq!(pool.failed_tasks(), 0);
    for id in [0, 1] {
        assert_eq!(mailbox.take(id), Some((TaskStatus::Completed, Some(id))));
        mailbox.assert_delivered_once(id);
    }
    let events = flush(pool.hooks(), &log).await;
    assert_eq!(
        kinds(&events, 1),
        vec!["submitted", "queued", "started", "completed"]
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, LifecycleEvent::Expired { .. }))
    );
}
