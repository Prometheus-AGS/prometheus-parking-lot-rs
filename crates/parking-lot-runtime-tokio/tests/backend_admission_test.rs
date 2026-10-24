//! OA-09: pre-filled adapter tasks have terminal outcomes and ledger ownership.
//!
//! Real `ResourcePool` submit/shutdown paths use the real dispatcher, queue,
//! mailbox, hook bus and `TokioSpawner`. Under the `oa-09` negative control,
//! an impossible head blocks the valid followers and the bounded oracle fails.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;

use prometheus_parking_lot_core::hooks::RejectionReason;
use prometheus_parking_lot_core::{
    Hook, HookBus, InMemoryMailbox, InMemoryQueue, LifecycleEvent, Mailbox, MailboxKey, PoolLimits,
    Priority, RecordingHook, ResourceCost, ResourceKind, ScheduledTask, SchedulerError, TaskError,
    TaskExecutor, TaskMetadata, TaskQueue, TaskStatus, TestClock,
};
use prometheus_parking_lot_runtime_tokio::{ResourcePool, ShutdownPolicy, TokioSpawner};

const WAIT: Duration = Duration::from_secs(5);
const DRAIN: Duration = Duration::from_secs(60);

#[derive(Default)]
struct Activity {
    runs: Vec<u64>,
    total: u32,
    gpu: u32,
    peak_total: u32,
    peak_gpu: u32,
}

struct Active {
    activity: Arc<Mutex<Activity>>,
    total: u32,
    gpu: u32,
}

impl Drop for Active {
    fn drop(&mut self) {
        let mut activity = self.activity.lock();
        activity.total -= self.total;
        activity.gpu -= self.gpu;
    }
}

#[derive(Clone)]
struct Executor(Arc<Mutex<Activity>>);

#[async_trait]
impl TaskExecutor<u64, u64> for Executor {
    async fn execute(&self, payload: u64, meta: TaskMetadata) -> Result<u64, TaskError> {
        let total = meta.cost.total_units();
        let gpu = meta.cost.units_of(ResourceKind::GpuVram);
        {
            let mut activity = self.0.lock();
            activity.runs.push(meta.id);
            activity.total += total;
            activity.gpu += gpu;
            activity.peak_total = activity.peak_total.max(activity.total);
            activity.peak_gpu = activity.peak_gpu.max(activity.gpu);
            assert!(activity.total <= 2, "active total never exceeds max_units");
            assert!(activity.gpu <= 1, "active GPU cost never exceeds its limit");
            drop(activity);
        }
        let _active = Active {
            activity: Arc::clone(&self.0),
            total,
            gpu,
        };
        tokio::task::yield_now().await;
        Ok(payload)
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
    fn take_once(&self, id: u64) -> (TaskStatus, Option<u64>) {
        let key = key(id);
        assert_eq!(
            self.deliveries.lock().iter().filter(|k| **k == key).count(),
            1
        );
        let mut stored = self.stored.lock();
        let result = stored
            .consume(&key)
            .expect("consume")
            .expect("terminal mailbox outcome");
        assert!(stored.consume(&key).expect("consume again").is_none());
        drop(stored);
        result
    }
}

fn key(id: u64) -> MailboxKey {
    MailboxKey {
        tenant: "oa-09".into(),
        user_id: None,
        session_id: Some(id.to_string()),
    }
}

fn task(id: u64, cost: ResourceCost) -> ScheduledTask<u64> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(key(id)),
            priority: Priority::Normal,
            cost,
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
        },
        payload: id,
    }
}

fn valid_cost() -> ResourceCost {
    ResourceCost::new(ResourceKind::GpuVram, 1).with(ResourceKind::Cpu, 1)
}

type Pool = ResourcePool<u64, u64, InMemoryQueue<u64>, SharedMailbox, Executor, TokioSpawner>;

fn pool(
    backend: InMemoryQueue<u64>,
    mailbox: &SharedMailbox,
    activity: &Arc<Mutex<Activity>>,
    hook: &Arc<RecordingHook>,
) -> Pool {
    ResourcePool::new(
        PoolLimits {
            max_units: 2,
            max_queue_depth: 4,
            default_timeout: WAIT,
        },
        backend,
        mailbox.clone(),
        Executor(Arc::clone(activity)),
        TokioSpawner::new(tokio::runtime::Handle::current()),
    )
    .with_name("oa-09")
    .with_resource_limit(ResourceKind::GpuVram, 1)
    .with_max_head_skips(0)
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

fn assert_terminal_once(events: &[LifecycleEvent], id: u64) {
    assert_eq!(
        events
            .iter()
            .filter(|event| event.task_id() == id && event.is_terminal())
            .count(),
        1,
        "{events:?}"
    );
}

fn assert_rejected(events: &[LifecycleEvent], id: u64, kind: Option<ResourceKind>) {
    assert_terminal_once(events, id);
    assert!(
        !events.iter().any(|event| {
            event.task_id() == id && matches!(event, LifecycleEvent::Started { .. })
        })
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            LifecycleEvent::Rejected {
                task_id,
                reason: RejectionReason::CapacityExceeded { kind: observed },
                ..
            } if *task_id == id && *observed == kind
        )),
        "{events:?}"
    );
}

#[tokio::test]
async fn backend_admission_prefilled_impossible_heads_do_not_block_submit_or_fifo() {
    let mut backend = InMemoryQueue::new(4);
    for item in [
        task(0, ResourceCost::new(ResourceKind::Cpu, 3)),
        task(1, ResourceCost::new(ResourceKind::GpuVram, 2)),
        task(2, valid_cost()),
        task(3, valid_cost()),
    ] {
        backend.enqueue(item).expect("prefill");
    }
    let mailbox = SharedMailbox::default();
    let activity = Arc::new(Mutex::new(Activity::default()));
    let hook = Arc::new(RecordingHook::new(32));
    let pool = pool(backend, &mailbox, &activity, &hook);
    // The initial backend is at depth. First observation must remove its
    // impossible heads before the new task is subjected to the depth bound.
    pool.submit(task(4, valid_cost()))
        .await
        .expect("valid new submission");
    assert_eq!(pool.active_units(), 2);
    assert_eq!(pool.active_units_of(ResourceKind::GpuVram), Some(1));
    assert_eq!(
        pool.queued_tasks(),
        2,
        "valid tasks wait for temporary capacity"
    );
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("valid followers must finish before the drain deadline");
    assert_eq!(
        (report.completed, report.cancelled, report.unsettled),
        (3, 0, 0)
    );
    assert_eq!((pool.completed_tasks(), pool.failed_tasks()), (3, 2));
    assert_eq!((pool.active_units(), pool.queued_tasks()), (0, 0));
    assert_eq!(pool.active_units_of(ResourceKind::GpuVram), Some(0));
    assert_eq!(activity.lock().runs, vec![2, 3, 4]);
    {
        let activity = activity.lock();
        assert_eq!((activity.peak_total, activity.peak_gpu), (2, 1));
    }
    assert_eq!(
        mailbox.take_once(0),
        (
            TaskStatus::Dropped(SchedulerError::capacity_exceeded("oa-09", None, 3, 2).to_string()),
            None
        )
    );
    assert_eq!(
        mailbox.take_once(1),
        (
            TaskStatus::Dropped(
                SchedulerError::capacity_exceeded("oa-09", Some(ResourceKind::GpuVram), 2, 1)
                    .to_string()
            ),
            None
        )
    );
    for id in 2..=4 {
        assert_eq!(mailbox.take_once(id), (TaskStatus::Completed, Some(id)));
    }
    let events = events(pool.hooks(), &hook).await;
    assert_rejected(&events, 0, None);
    assert_rejected(&events, 1, Some(ResourceKind::GpuVram));
    for id in 2..=4 {
        assert_terminal_once(&events, id);
    }
}

#[tokio::test]
async fn backend_admission_shutdown_first_accounts_for_prefilled_expiry_rejection_and_run() {
    let mut backend = InMemoryQueue::new(4);
    let mut expired = task(0, valid_cost());
    expired.meta.deadline_ms = Some(9);
    for item in [
        expired,
        task(1, ResourceCost::new(ResourceKind::Cpu, 3)),
        task(2, valid_cost()),
    ] {
        backend.enqueue(item).expect("prefill");
    }
    let mailbox = SharedMailbox::default();
    let activity = Arc::new(Mutex::new(Activity::default()));
    let hook = Arc::new(RecordingHook::new(16));
    let pool = pool(backend, &mailbox, &activity, &hook).with_clock(TestClock::new(10));
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("prefilled tasks must be accounted even without a submit");
    assert_eq!(
        (report.completed, report.cancelled, report.unsettled),
        (3, 0, 0)
    );
    assert_eq!((pool.completed_tasks(), pool.failed_tasks()), (1, 2));
    assert_eq!(pool.active_units(), 0);
    assert_eq!(activity.lock().runs, vec![2]);
    assert_eq!(mailbox.take_once(0), (TaskStatus::Expired, None));
    assert!(
        matches!(mailbox.take_once(1), (TaskStatus::Dropped(reason), None) if reason.contains("capacity exceeded"))
    );
    assert_eq!(mailbox.take_once(2), (TaskStatus::Completed, Some(2)));
    let events = events(pool.hooks(), &hook).await;
    for id in 0..3 {
        assert_terminal_once(&events, id);
    }
    assert_rejected(&events, 1, None);
}

#[tokio::test]
async fn backend_admission_valid_prefilled_fifo_and_temporary_capacity_are_preserved() {
    let mut backend = InMemoryQueue::new(4);
    // Deliberately supply ids out of order; stored submission sequences
    // establish the FIFO that the adapter must preserve.
    for (sequence, id) in [(10, 30), (20, 10), (30, 20)] {
        let mut item = task(id, valid_cost());
        item.meta.sequence = sequence;
        backend.enqueue(item).expect("prefill");
    }
    let mailbox = SharedMailbox::default();
    let activity = Arc::new(Mutex::new(Activity::default()));
    let hook = Arc::new(RecordingHook::new(16));
    let pool = pool(backend, &mailbox, &activity, &hook);
    let report = tokio::time::timeout(
        WAIT,
        pool.shutdown_with(ShutdownPolicy::Drain { deadline: DRAIN }),
    )
    .await
    .expect("valid stored FIFO must drain");
    assert_eq!(
        (report.completed, report.cancelled, report.unsettled),
        (3, 0, 0)
    );
    assert_eq!((pool.completed_tasks(), pool.failed_tasks()), (3, 0));
    assert_eq!(activity.lock().runs, vec![30, 10, 20]);
    let events = events(pool.hooks(), &hook).await;
    for id in [30, 10, 20] {
        assert_eq!(mailbox.take_once(id), (TaskStatus::Completed, Some(id)));
        assert_terminal_once(&events, id);
    }
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, LifecycleEvent::Rejected { .. }))
    );
}
