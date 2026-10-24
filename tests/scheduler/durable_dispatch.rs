//! Durable dispatch on the in-memory reference backends: the persist, lease,
//! execute, deliver, ack ordering and its outcome mapping, observed through
//! recording wrappers around `InMemoryLeaseQueue` and `InMemoryResultMailbox`.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;
use prometheus_parking_lot::core::{
    Delivery, DeliveryOutcome, Enqueued, LeaseQueue, LeaseToken, LeasedTask, MailboxEntry,
    NackAction, QueueDepth, ResultMailbox, ScheduledTask, SchedulerError, ShutdownPolicy,
    TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::mailbox::InMemoryResultMailbox;
use prometheus_parking_lot::infra::queue::InMemoryLeaseQueue;
use prometheus_parking_lot::util::{MailboxKey, Priority};
use prometheus_parking_lot::{DurableOptions, DurableResourcePool};

use crate::support::{Behaviour, Executor, eventually, key, limits, spawner, task};

const LEASE: Duration = Duration::from_millis(600);

fn options() -> DurableOptions {
    DurableOptions::new(LEASE)
        .with_name("durable")
        .with_poll_interval(Duration::from_millis(20))
}

/// One storage operation, in the order the pool performed it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Event {
    Enqueued,
    Delivered { key: String, status: String },
    Acked { entry: u64 },
    Nacked { entry: u64, action: NackAction },
}

type Log = Arc<Mutex<Vec<Event>>>;

/// A lease queue that records enqueue, ack and nack. `drop_first_ack` makes
/// the first ack report success without reaching the queue: the shape of a
/// process that died after the delivery and before its ack landed.
struct RecordingQueue {
    inner: InMemoryLeaseQueue<String>,
    log: Log,
    drop_first_ack: AtomicBool,
    fail_enqueue: bool,
}

impl RecordingQueue {
    fn new(log: &Log) -> Self {
        Self {
            inner: InMemoryLeaseQueue::new(100),
            log: Arc::clone(log),
            drop_first_ack: AtomicBool::new(false),
            fail_enqueue: false,
        }
    }
}

impl LeaseQueue<String> for RecordingQueue {
    async fn enqueue(&self, task: ScheduledTask<String>) -> Result<Enqueued, SchedulerError> {
        if self.fail_enqueue {
            return Err(SchedulerError::internal("store unavailable"));
        }
        let enqueued = self.inner.enqueue(task).await?;
        self.log.lock().push(Event::Enqueued);
        Ok(enqueued)
    }

    fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> impl Future<Output = Result<Vec<LeasedTask<String>>, SchedulerError>> + Send {
        self.inner.lease(max, lease_for)
    }

    async fn ack(&self, token: LeaseToken) -> Result<(), SchedulerError> {
        if self.drop_first_ack.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        self.inner.ack(token).await?;
        self.log.lock().push(Event::Acked {
            entry: token.entry().0,
        });
        Ok(())
    }

    async fn nack(&self, token: LeaseToken, action: NackAction) -> Result<(), SchedulerError> {
        self.inner.nack(token, action.clone()).await?;
        self.log.lock().push(Event::Nacked {
            entry: token.entry().0,
            action,
        });
        Ok(())
    }

    fn extend(
        &self,
        token: LeaseToken,
        lease_for: Duration,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.inner.extend(token, lease_for)
    }

    fn depth(&self) -> impl Future<Output = Result<QueueDepth, SchedulerError>> + Send {
        self.inner.depth()
    }

    fn max_depth(&self) -> usize {
        self.inner.max_depth()
    }

    fn identity(&self) -> &str {
        self.inner.identity()
    }
}

/// A result mailbox that records every delivery (stored or duplicate).
struct RecordingMailbox {
    inner: InMemoryResultMailbox<String>,
    log: Log,
}

impl ResultMailbox<String> for RecordingMailbox {
    async fn deliver(&self, delivery: Delivery<String>) -> Result<DeliveryOutcome, SchedulerError> {
        let key = delivery.idempotency_key.clone().unwrap_or_default();
        let status = format!("{:?}", delivery.status);
        let outcome = self.inner.deliver(delivery).await?;
        self.log.lock().push(Event::Delivered { key, status });
        Ok(outcome)
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.inner.evict_expired()
    }
}

fn recording(log: &Log) -> (RecordingQueue, RecordingMailbox) {
    (
        RecordingQueue::new(log),
        RecordingMailbox {
            inner: InMemoryResultMailbox::new(),
            log: Arc::clone(log),
        },
    )
}

/// Index of the first event matching `pred`.
fn position(log: &[Event], pred: impl Fn(&Event) -> bool) -> Option<usize> {
    log.iter().position(pred)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_ack_after_delivery_orders_every_outcome() {
    let log = Log::default();
    let (queue, mailbox) = recording(&log);
    let pool = DurableResourcePool::start(
        limits(4),
        queue,
        mailbox,
        Executor::new(Behaviour::Succeed),
        spawner(),
        options(),
    )
    .expect("start");
    for (session, payload) in [("ok", "ok-1"), ("fail", "fail-1"), ("panic", "panic-1")] {
        assert_eq!(
            pool.submit(task(session, payload)).await.expect("submit"),
            TaskStatus::Queued
        );
    }

    let resolved = eventually(Duration::from_secs(10), || {
        let log = Arc::clone(&log);
        async move {
            let snapshot = log.lock().clone();
            let count = snapshot
                .iter()
                .filter(|e| matches!(e, Event::Acked { .. } | Event::Nacked { .. }))
                .count();
            (count == 3).then_some(snapshot)
        }
    })
    .await
    .expect("every task resolves its lease");

    // Entries are 1, 2, 3 in submission order; without a caller key each
    // delivery is keyed by the queue's identity and its entry.
    let identity = pool.queue().identity().to_owned();
    assert!(identity.starts_with("in-memory:"), "{identity}");
    for (entry, acked) in [(1_u64, true), (2, false), (3, false)] {
        let delivery_key = format!("{identity}:entry:{entry}");
        let delivered = position(
            &resolved,
            |e| matches!(e, Event::Delivered { key, .. } if *key == delivery_key),
        )
        .unwrap_or_else(|| panic!("entry {entry} was never delivered: {resolved:?}"));
        let resolved_at = position(&resolved, |e| match e {
            Event::Acked { entry: id } => *id == entry && acked,
            Event::Nacked { entry: id, action } => {
                *id == entry && !acked && matches!(action, NackAction::DeadLetter { .. })
            }
            _ => false,
        })
        .unwrap_or_else(|| panic!("entry {entry} was not resolved as expected: {resolved:?}"));
        assert!(
            delivered < resolved_at,
            "entry {entry}'s lease was resolved before its outcome was delivered: {resolved:?}"
        );
    }

    let outcome = |session: &'static str| {
        let mailbox = pool.mailbox();
        async move {
            mailbox
                .consume(&key(session))
                .await
                .expect("consume")
                .expect("an outcome")
                .status
        }
    };
    assert_eq!(outcome("ok").await, TaskStatus::Completed);
    assert!(matches!(
        outcome("fail").await,
        TaskStatus::Failed(TaskError::Executor { .. })
    ));
    assert!(matches!(
        outcome("panic").await,
        TaskStatus::Failed(TaskError::ExecutorPanicked { .. })
    ));
    assert_eq!(pool.completed_tasks(), 1);
    assert_eq!(pool.failed_tasks(), 2);
    let depth = pool.queue().depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 2), "{depth:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_submit_persists_before_accepting() {
    let log = Log::default();
    let (mut queue, mailbox) = recording(&log);
    queue.fail_enqueue = true;
    let executor = Executor::new(Behaviour::Succeed);
    let pool = DurableResourcePool::start(
        limits(4),
        queue,
        mailbox,
        executor.clone(),
        spawner(),
        options(),
    )
    .expect("start");

    // The store refuses the task: submit fails and nothing runs.
    let refused = pool.submit(task("refused", "work")).await;
    assert!(
        matches!(refused, Err(SchedulerError::Internal { .. })),
        "{refused:?}"
    );
    tokio::time::sleep(LEASE).await;
    assert_eq!(executor.runs(), 0);
    assert!(log.lock().is_empty(), "{:?}", log.lock());

    // Rejected before anything is stored: a cost the pool can never admit.
    let mut huge = task("huge", "work");
    huge.meta.cost = prometheus_parking_lot::util::ResourceCost::new(
        prometheus_parking_lot::util::ResourceKind::Cpu,
        5,
    );
    assert!(matches!(
        pool.submit(huge).await,
        Err(SchedulerError::CapacityExceeded { requested: 5, .. })
    ));

    // After shutdown nothing is accepted.
    pool.shutdown();
    assert!(matches!(
        pool.submit(task("late", "work")).await,
        Err(SchedulerError::PoolShutdown { .. })
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_redelivered_task_delivers_exactly_one_result() {
    let log = Log::default();
    let (queue, mailbox) = recording(&log);
    // The first execution delivers, but its ack never lands (as if the process
    // died right after the delivery): the lease expires and the task runs again.
    queue.drop_first_ack.store(true, Ordering::SeqCst);
    let executor = Executor::new(Behaviour::Succeed);
    let pool = DurableResourcePool::start(
        limits(4),
        queue,
        mailbox,
        executor.clone(),
        spawner(),
        options(),
    )
    .expect("start");
    pool.submit(task("redelivered", "work"))
        .await
        .expect("submit");

    let acked = eventually(Duration::from_secs(10), || {
        let log = Arc::clone(&log);
        async move {
            let snapshot = log.lock().clone();
            snapshot
                .iter()
                .any(|e| matches!(e, Event::Acked { .. }))
                .then_some(snapshot)
        }
    })
    .await
    .expect("the redelivered task is acked");

    assert_eq!(
        executor.runs(),
        2,
        "the task ran again after its lease expired"
    );
    let deliveries: Vec<_> = acked
        .iter()
        .filter_map(|e| match e {
            Event::Delivered { key, .. } => Some(key.as_str()),
            _ => None,
        })
        .collect();
    let entry_key = format!("{}:entry:1", pool.queue().identity());
    assert_eq!(
        deliveries,
        [entry_key.as_str(), entry_key.as_str()],
        "both executions deliver under the entry's key"
    );
    let mailbox = pool.mailbox();
    assert!(
        mailbox
            .consume(&key("redelivered"))
            .await
            .expect("consume")
            .is_some()
    );
    assert!(
        mailbox
            .consume(&key("redelivered"))
            .await
            .expect("consume")
            .is_none(),
        "the second execution's result was deduplicated"
    );
    assert_eq!(pool.queue().depth().await.expect("depth").live(), 0);
}

/// Records the order tasks start in and the most units held at once.
#[derive(Clone, Default)]
struct Gauge {
    running: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
    order: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl TaskExecutor<String, String> for Gauge {
    async fn execute(&self, payload: String, meta: TaskMetadata) -> Result<String, TaskError> {
        let units = meta.cost.total_units();
        let now = self.running.fetch_add(units, Ordering::SeqCst) + units;
        self.peak.fetch_max(now, Ordering::SeqCst);
        self.order.lock().push(payload.clone());
        tokio::time::sleep(Duration::from_millis(40)).await;
        self.running.fetch_sub(units, Ordering::SeqCst);
        Ok(payload)
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn durable_capacity_and_priority_then_fifo_order_hold() {
    let log = Log::default();
    let (queue, mailbox) = recording(&log);
    // Enqueued before the pool starts, as a restarted process finds them.
    for (payload, priority) in [
        ("low-1", Priority::Low),
        ("high-1", Priority::High),
        ("normal-1", Priority::Normal),
        ("high-2", Priority::High),
        ("low-2", Priority::Low),
    ] {
        let mut queued = task(payload, payload);
        queued.meta.priority = priority;
        let _ = queue.enqueue(queued).await.expect("enqueue");
    }
    let gauge = Gauge::default();
    let pool = DurableResourcePool::start(
        limits(1),
        queue,
        mailbox,
        gauge.clone(),
        spawner(),
        options(),
    )
    .expect("start");
    // Submitted after the start at the lowest priority: they run last, in
    // submission order, whenever they land.
    for i in 0..6 {
        let mut more = task(&format!("more-{i}"), &format!("more-{i}"));
        more.meta.priority = Priority::Low;
        pool.submit(more).await.expect("submit");
    }

    let drained = eventually(Duration::from_secs(20), || async {
        let depth = pool.queue().depth().await.expect("depth");
        (depth.live() == 0).then_some(())
    })
    .await;
    assert!(drained.is_some(), "the queue never drained");
    assert_eq!(
        gauge.peak.load(Ordering::SeqCst),
        1,
        "capacity of 1 unit exceeded"
    );
    let order = gauge.order.lock().clone();
    assert_eq!(
        order,
        [
            "high-1", "high-2", "normal-1", "low-1", "low-2", "more-0", "more-1", "more-2",
            "more-3", "more-4", "more-5"
        ],
        "leased in priority, then enqueue, order"
    );
}

/// Counts the tasks executing at once.
#[derive(Clone, Default)]
struct PeakExecutor {
    running: Arc<AtomicU32>,
    peak: Arc<AtomicU32>,
    done: Arc<AtomicU32>,
}

#[async_trait]
impl TaskExecutor<String, String> for PeakExecutor {
    async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        let now = self.running.fetch_add(1, Ordering::SeqCst) + 1;
        self.peak.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(150)).await;
        self.running.fetch_sub(1, Ordering::SeqCst);
        self.done.fetch_add(1, Ordering::SeqCst);
        Ok(payload)
    }
}

/// cr-23 on the durable pool: with 8 VRAM units per pool (and 100 total
/// units), two tasks of 8 VRAM never run together although the total fits,
/// both still run, and a task over the VRAM limit is rejected at submission.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_admits_by_every_dimension() {
    let executor = PeakExecutor::default();
    let pool = DurableResourcePool::start(
        limits(100),
        InMemoryLeaseQueue::<String>::new(100),
        InMemoryResultMailbox::<String>::new(),
        executor.clone(),
        spawner(),
        options().with_resource_limit(prometheus_parking_lot::util::ResourceKind::GpuVram, 8),
    )
    .expect("start");
    let vram_task = |session: &str, vram: u32| {
        let mut task = task(session, session);
        task.meta.cost = prometheus_parking_lot::util::ResourceCost::new(
            prometheus_parking_lot::util::ResourceKind::GpuVram,
            vram,
        )
        .with(prometheus_parking_lot::util::ResourceKind::Cpu, 1);
        task
    };
    let too_big = pool.submit(vram_task("big", 9)).await;
    assert!(
        matches!(
            too_big,
            Err(SchedulerError::CapacityExceeded {
                requested: 9,
                available: 8,
                ..
            })
        ),
        "{too_big:?}"
    );
    for session in ["a", "b"] {
        pool.submit(vram_task(session, 8)).await.expect("accepted");
    }
    let finished = eventually(Duration::from_secs(10), || {
        let done = Arc::clone(&executor.done);
        async move { (done.load(Ordering::SeqCst) == 2).then_some(()) }
    })
    .await;
    assert!(finished.is_some(), "both accepted tasks run");
    assert_eq!(
        executor.peak.load(Ordering::SeqCst),
        1,
        "two 8-unit VRAM tasks ran together under an 8-unit limit"
    );
}

// ── Review findings (0.3.0 gate) ────────────────────────────────────────────

/// One `InMemoryLeaseQueue` shared by several pools (as several processes
/// share one database queue). `lose_acks` reports every ack as done without
/// reaching the queue: the shape of a process that died after its delivery
/// and before its ack landed.
pub struct SharedQueue {
    pub inner: Arc<InMemoryLeaseQueue<String>>,
    pub lose_acks: bool,
}

impl LeaseQueue<String> for SharedQueue {
    fn enqueue(
        &self,
        task: ScheduledTask<String>,
    ) -> impl Future<Output = Result<Enqueued, SchedulerError>> + Send {
        self.inner.enqueue(task)
    }

    fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> impl Future<Output = Result<Vec<LeasedTask<String>>, SchedulerError>> + Send {
        self.inner.lease(max, lease_for)
    }

    async fn ack(&self, token: LeaseToken) -> Result<(), SchedulerError> {
        if self.lose_acks {
            return Ok(());
        }
        self.inner.ack(token).await
    }

    fn nack(
        &self,
        token: LeaseToken,
        action: NackAction,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.inner.nack(token, action)
    }

    fn extend(
        &self,
        token: LeaseToken,
        lease_for: Duration,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.inner.extend(token, lease_for)
    }

    fn depth(&self) -> impl Future<Output = Result<QueueDepth, SchedulerError>> + Send {
        self.inner.depth()
    }

    fn max_depth(&self) -> usize {
        self.inner.max_depth()
    }

    fn identity(&self) -> &str {
        self.inner.identity()
    }
}

/// One `InMemoryResultMailbox` shared by several pools. The first `fail`
/// deliveries fail; with `hold_first` set, the first delivery waits for
/// `release` (and sets `entered` when it starts waiting).
#[derive(Clone, Default)]
pub struct SharedMailbox {
    pub inner: Arc<InMemoryResultMailbox<String>>,
    fail: Arc<Mutex<u32>>,
    hold_first: Arc<AtomicBool>,
    entered: Arc<AtomicBool>,
    release: Arc<tokio::sync::Notify>,
}

impl SharedMailbox {
    fn failing(times: u32) -> Self {
        let mailbox = Self::default();
        *mailbox.fail.lock() = times;
        mailbox
    }

    /// Every result consumable for `session`, in order.
    async fn consume_all(&self, session: &str) -> Vec<MailboxEntry<String>> {
        let mut found = Vec::new();
        while let Some(entry) = self.inner.consume(&key(session)).await.expect("consume") {
            found.push(entry);
        }
        found
    }
}

impl ResultMailbox<String> for SharedMailbox {
    async fn deliver(&self, delivery: Delivery<String>) -> Result<DeliveryOutcome, SchedulerError> {
        if self.hold_first.swap(false, Ordering::SeqCst) {
            self.entered.store(true, Ordering::SeqCst);
            self.release.notified().await;
        }
        let failing = {
            let mut left = self.fail.lock();
            left.checked_sub(1).map(|rest| *left = rest).is_some()
        };
        if failing {
            return Err(SchedulerError::internal("injected delivery failure"));
        }
        self.inner.deliver(delivery).await
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.inner.evict_expired()
    }
}

/// C1: two consumers of one queue, named differently. "worker-a" delivers
/// the result, then dies before its ack lands; the redelivery goes to
/// "worker-b", which runs the task again. Exactly one result is consumable,
/// because the result key comes from the queue, not the pool name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_differently_named_consumers_of_one_queue_store_one_result() {
    let queue = Arc::new(InMemoryLeaseQueue::<String>::new(100));
    let mailbox = SharedMailbox::default();
    let worker_a = Executor::new(Behaviour::Succeed);
    let a = DurableResourcePool::start(
        limits(4),
        SharedQueue {
            inner: Arc::clone(&queue),
            lose_acks: true,
        },
        mailbox.clone(),
        worker_a.clone(),
        spawner(),
        options().with_name("worker-a"),
    )
    .expect("start worker-a");
    a.submit(task("job", "work")).await.expect("submit");
    let delivered = eventually(Duration::from_secs(30), || async {
        mailbox.inner.peek(&key("job")).await.expect("peek")
    })
    .await;
    assert!(delivered.is_some(), "worker-a never delivered");
    // worker-a "dies": it leases nothing more, and its lost ack leaves the
    // entry leased until the lease expires.
    a.shutdown();
    assert_eq!(worker_a.runs(), 1);

    let worker_b = Executor::new(Behaviour::Succeed);
    let b = DurableResourcePool::start(
        limits(4),
        SharedQueue {
            inner: Arc::clone(&queue),
            lose_acks: false,
        },
        mailbox.clone(),
        worker_b.clone(),
        spawner(),
        options().with_name("worker-b"),
    )
    .expect("start worker-b");
    let drained = eventually(Duration::from_secs(30), || async {
        let depth = b.queue().depth().await.expect("depth");
        (depth.live() == 0).then_some(())
    })
    .await;
    assert!(drained.is_some(), "worker-b never acked the redelivery");
    assert_eq!(worker_b.runs(), 1, "worker-b re-ran the redelivered task");

    let results = mailbox.consume_all("job").await;
    assert_eq!(
        results.len(),
        1,
        "one accepted task, consumable results: {results:?}"
    );
    assert_eq!(results[0].payload.as_deref(), Some("done:work"));
}

/// C1: two pools with the default name, on two different queues whose entry
/// ids both start at 1, deliver into one mailbox. Both results are kept.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_pools_on_two_queues_share_one_mailbox_and_keep_both_results() {
    let mailbox = SharedMailbox::default();
    let default_options =
        || DurableOptions::new(LEASE).with_poll_interval(Duration::from_millis(20));
    let first = DurableResourcePool::start(
        limits(4),
        InMemoryLeaseQueue::<String>::new(100),
        mailbox.clone(),
        Executor::new(Behaviour::Succeed),
        spawner(),
        default_options(),
    )
    .expect("start the first pool");
    let second = DurableResourcePool::start(
        limits(4),
        InMemoryLeaseQueue::<String>::new(100),
        mailbox.clone(),
        Executor::new(Behaviour::Succeed),
        spawner(),
        default_options(),
    )
    .expect("start the second pool");
    assert_eq!(first.options().name(), second.options().name());
    assert_ne!(first.queue().identity(), second.queue().identity());

    first
        .submit(task("first-job", "one"))
        .await
        .expect("submit");
    // In order: the first result is stored before the second is delivered.
    let stored = eventually(Duration::from_secs(30), || async {
        mailbox.inner.peek(&key("first-job")).await.expect("peek")
    })
    .await;
    assert!(stored.is_some(), "the first result never arrived");
    second
        .submit(task("second-job", "two"))
        .await
        .expect("submit");
    let second_result = eventually(Duration::from_secs(30), || async {
        mailbox
            .inner
            .consume(&key("second-job"))
            .await
            .expect("consume")
    })
    .await
    .expect("the second pool's result was dropped as a duplicate");
    assert_eq!(second_result.payload.as_deref(), Some("done:two"));
    let first_results = mailbox.consume_all("first-job").await;
    assert_eq!(first_results.len(), 1, "{first_results:?}");
}

/// W1: a task whose outcome cannot be stored is handed back, and the
/// shutdown report counts it as requeued (not completed).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_shutdown_report_counts_an_undeliverable_task_as_requeued() {
    // Five failures: every delivery attempt of the first run fails.
    let mailbox = SharedMailbox::failing(5);
    let executor = Executor::new(Behaviour::SleepThenSucceed(Duration::from_millis(300)));
    let pool = DurableResourcePool::start(
        limits(2),
        InMemoryLeaseQueue::<String>::new(100),
        mailbox,
        executor.clone(),
        spawner(),
        DurableOptions::new(Duration::from_secs(30)).with_poll_interval(Duration::from_millis(20)),
    )
    .expect("start");
    pool.submit(task("a1", "work")).await.expect("submit");
    let started = eventually(Duration::from_secs(30), || async {
        (executor.runs() == 1).then_some(())
    })
    .await;
    assert!(started.is_some(), "the task never started");

    let report = pool
        .shutdown_with(ShutdownPolicy::DrainThenCancel {
            deadline: Duration::from_secs(30),
        })
        .await;
    assert_eq!(
        (
            report.completed,
            report.cancelled,
            report.requeued,
            report.unsettled
        ),
        (0, 1, 1, 0),
        "{report:?}"
    );
    let depth = pool.queue().depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (1, 0), "{depth:?}");
}

/// W1: a task whose executor returned before the shutdown began, but whose
/// delivery is still in progress, is counted as completed when it lands.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_shutdown_report_counts_a_task_still_delivering_as_completed() {
    let mailbox = SharedMailbox::default();
    mailbox.hold_first.store(true, Ordering::SeqCst);
    let pool = DurableResourcePool::start(
        limits(2),
        InMemoryLeaseQueue::<String>::new(100),
        mailbox.clone(),
        Executor::new(Behaviour::Succeed),
        spawner(),
        DurableOptions::new(Duration::from_secs(30)).with_poll_interval(Duration::from_millis(20)),
    )
    .expect("start");
    pool.submit(task("a1", "work")).await.expect("submit");
    let delivering = eventually(Duration::from_secs(30), || async {
        mailbox.entered.load(Ordering::SeqCst).then_some(())
    })
    .await;
    assert!(delivering.is_some(), "the delivery never started");

    // The shutdown snapshots its counters on its first poll, before the
    // delivery is released.
    let release = Arc::clone(&mailbox.release);
    let (report, ()) = tokio::join!(
        pool.shutdown_with(ShutdownPolicy::DrainThenCancel {
            deadline: Duration::from_secs(30),
        }),
        async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            release.notify_one();
        }
    );
    assert_eq!(
        (
            report.completed,
            report.cancelled,
            report.requeued,
            report.unsettled
        ),
        (1, 0, 0, 0),
        "{report:?}"
    );
    assert_eq!(mailbox.consume_all("a1").await.len(), 1);
}

/// W4: a leased task too large for this pool is handed back (delayed), not
/// dead-lettered, and a larger consumer of the same queue then runs it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_task_too_large_for_one_consumer_runs_on_another() {
    let queue = Arc::new(InMemoryLeaseQueue::<String>::new(100));
    let mailbox = SharedMailbox::default();
    let mut big = task("big", "big");
    big.meta.cost = prometheus_parking_lot::util::ResourceCost::new(
        prometheus_parking_lot::util::ResourceKind::Cpu,
        2,
    );
    let _ = queue.enqueue(big).await.expect("enqueue");
    let shared = |queue: &Arc<InMemoryLeaseQueue<String>>| SharedQueue {
        inner: Arc::clone(queue),
        lose_acks: false,
    };

    let small_runs = Executor::new(Behaviour::Succeed);
    let small = DurableResourcePool::start(
        limits(1),
        shared(&queue),
        mailbox.clone(),
        small_runs.clone(),
        spawner(),
        options().with_name("small"),
    )
    .expect("start the small pool");
    let handed_back = eventually(Duration::from_secs(30), || async {
        let depth = queue.depth().await.expect("depth");
        (depth.delayed == 1).then_some(depth)
    })
    .await;
    let depth = queue.depth().await.expect("depth");
    assert!(
        handed_back.is_some(),
        "the small pool did not hand the task back: {depth:?}"
    );
    assert_eq!(depth.dead_lettered, 0, "accepted work was destroyed");
    small.shutdown();
    assert_eq!(small_runs.runs(), 0);

    let large_runs = Executor::new(Behaviour::Succeed);
    let _large = DurableResourcePool::start(
        limits(4),
        shared(&queue),
        mailbox.clone(),
        large_runs.clone(),
        spawner(),
        options().with_name("large"),
    )
    .expect("start the large pool");
    let result = eventually(Duration::from_secs(30), || async {
        mailbox.inner.consume(&key("big")).await.expect("consume")
    })
    .await
    .expect("the large pool never ran the task");
    assert_eq!(result.status, TaskStatus::Completed);
    assert_eq!(large_runs.runs(), 1);
    let depth = queue.depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 0), "{depth:?}");
}

/// W5: zero-cost tasks take no unit, yet the number running at once is
/// bounded: by `max_units` by default, by `with_max_running` when set.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn durable_zero_cost_tasks_are_bounded() {
    for (max_units, max_running, bound) in [(2_u32, None, 2_u32), (4, Some(1_u32), 1)] {
        let executor = PeakExecutor::default();
        let mut opts = options();
        if let Some(max) = max_running {
            opts = opts.with_max_running(max);
        }
        let pool = DurableResourcePool::start(
            limits(max_units),
            InMemoryLeaseQueue::<String>::new(100),
            InMemoryResultMailbox::<String>::new(),
            executor.clone(),
            spawner(),
            opts,
        )
        .expect("start");
        for i in 0..6 {
            let mut free = task(&format!("free-{i}"), "free");
            free.meta.cost = prometheus_parking_lot::util::ResourceCost::new(
                prometheus_parking_lot::util::ResourceKind::Cpu,
                0,
            );
            pool.submit(free).await.expect("accepted");
        }
        let finished = eventually(Duration::from_secs(30), || {
            let done = Arc::clone(&executor.done);
            async move { (done.load(Ordering::SeqCst) == 6).then_some(()) }
        })
        .await;
        assert!(finished.is_some(), "every zero-cost task runs");
        let peak = executor.peak.load(Ordering::SeqCst);
        assert!(
            (1..=bound).contains(&peak),
            "{peak} zero-cost tasks ran at once ({max_units} units, max_running {max_running:?})"
        );
    }
}

/// An executor whose future panics when it is dropped (cancelled).
#[derive(Clone)]
struct PanicsWhenCancelled;

struct DropBomb;

impl Drop for DropBomb {
    fn drop(&mut self) {
        assert!(
            std::thread::panicking(),
            "executor future panicked while being dropped"
        );
    }
}

#[async_trait]
impl TaskExecutor<String, String> for PanicsWhenCancelled {
    async fn execute(&self, _payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        let _bomb = DropBomb;
        std::future::pending::<()>().await;
        Ok(String::new())
    }
}

/// S1: a running task cancelled at shutdown whose executor panics while it
/// is dropped is handed back, not dead-lettered as failed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn durable_cancelled_task_that_panics_on_drop_is_requeued() {
    let mailbox = SharedMailbox::default();
    let pool = DurableResourcePool::start(
        limits(2),
        InMemoryLeaseQueue::<String>::new(100),
        mailbox.clone(),
        PanicsWhenCancelled,
        spawner(),
        options(),
    )
    .expect("start");
    pool.submit(task("bomb", "bomb")).await.expect("submit");
    let running = eventually(Duration::from_secs(30), || async {
        (pool.running_tasks() == 1).then_some(())
    })
    .await;
    assert!(running.is_some(), "the task never started");
    let report = pool.shutdown_with(ShutdownPolicy::CancelImmediately).await;
    assert_eq!(
        (report.completed, report.cancelled, report.requeued),
        (0, 1, 1),
        "{report:?}"
    );
    let depth = pool.queue().depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (1, 0), "{depth:?}");
    assert_eq!(mailbox.consume_all("bomb").await.len(), 0);
}
