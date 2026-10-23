//! Regression tests for the 0.3.0 phase-boundary review of the dispatcher,
//! semaphore, in-memory queue and background notifier (W1, W3, S1 to S6).
//!
//! Each test drives the public API and asserts the behaviour the review asked
//! for; on the code the review read, every one of them fails.

use std::future::Future;
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use prometheus_parking_lot_core::dispatch::{
    Dispatcher, Grant, Parkable, ParkedQueue, PriorityQueue, Ticket,
};
use prometheus_parking_lot_core::mailbox::{
    BackgroundNotifier, DetachedNotifier, RetryPolicy, Sleeper,
};
use prometheus_parking_lot_core::semaphore::{UnitSemaphore, UnitVector};
use prometheus_parking_lot_core::{
    Delivery, InMemoryQueue, MailboxEntry, MailboxKey, Notifier, Priority, ResourceCost,
    ResourceKind, ScheduledTask, SchedulerError, Spawn, TaskMetadata, TaskQueue, TaskStatus,
};

#[derive(Debug)]
struct Item {
    name: &'static str,
    priority: Priority,
    demand: UnitVector,
    deadline_ms: Option<u128>,
    sequence: u64,
}

impl Parkable for Item {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: self.priority,
            sequence: self.sequence,
            demand: self.demand,
            deadline_ms: self.deadline_ms,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.sequence = sequence;
    }
}

const fn item(name: &'static str, priority: Priority, units: u32) -> Item {
    Item {
        name,
        priority,
        demand: UnitVector::scalar(units),
        deadline_ms: None,
        sequence: 0,
    }
}

fn costed(name: &'static str, cost: &ResourceCost) -> Item {
    Item {
        demand: UnitVector::from_cost(cost),
        ..item(name, Priority::Normal, 0)
    }
}

fn names(grants: &[Grant<Item>]) -> Vec<&'static str> {
    grants.iter().map(|grant| grant.item.name).collect()
}

/// 4 units, depth 1: a 2-unit blocker runs and a 4-unit head is parked, so
/// the queue is at its depth limit with 2 units free.
fn at_depth() -> (Dispatcher<Item>, Vec<Grant<Item>>) {
    let dispatcher = Dispatcher::in_memory("review", UnitSemaphore::new(4), 1);
    let blocker = dispatcher
        .submit(item("blocker", Priority::Normal, 2), 0)
        .expect("accepted");
    assert!(blocker.started);
    let head = dispatcher
        .submit(item("head", Priority::High, 4), 0)
        .expect("accepted");
    assert!(!head.started);
    assert_eq!(dispatcher.parked(), 1);
    (dispatcher, blocker.dispatched.started)
}

/// W1: at depth, an item that fits beside the blocked head skips ahead of
/// it instead of being refused with `QueueFull`.
#[test]
fn review_w1_item_that_can_skip_ahead_is_admitted_at_depth() {
    let (dispatcher, running) = at_depth();
    let small = dispatcher.submit(item("small", Priority::Low, 1), 0);
    let small = small.expect("a 1-unit item fits now: it must not get QueueFull");
    assert!(small.started);
    assert_eq!(names(&small.dispatched.started), vec!["small"]);
    assert_eq!(dispatcher.skip_ahead_stats().skipped, 1);
    assert_eq!(dispatcher.parked(), 1, "the head is still parked");
    assert_eq!(dispatcher.units_in_use(), 3);
    drop((running, small));
}

/// W1: at depth, a higher-priority item that fits becomes the head the drain
/// would serve and starts; it is not a skip.
#[test]
fn review_w1_higher_priority_item_that_fits_is_admitted_at_depth() {
    let (dispatcher, running) = at_depth();
    let urgent = dispatcher
        .submit(item("urgent", Priority::Critical, 1), 0)
        .expect("a Critical 1-unit item fits now: it must not get QueueFull");
    assert!(urgent.started);
    assert_eq!(dispatcher.skip_ahead_stats().skipped, 0);
    drop((running, urgent));
}

/// W1: the at-depth start keeps the starvation guard: once the head has
/// been skipped `max_head_skips` times, an item that fits is refused at depth
/// (it would have to wait), and only an item that must wait is refused.
#[test]
fn review_w1_at_depth_start_respects_the_guard_and_refuses_waiting_items() {
    let dispatcher =
        Dispatcher::in_memory("review", UnitSemaphore::new(4), 1).with_max_head_skips(1);
    let blocker = dispatcher
        .submit(item("blocker", Priority::Normal, 2), 0)
        .expect("accepted");
    let _ = dispatcher
        .submit(item("head", Priority::High, 4), 0)
        .expect("accepted");
    let first = dispatcher
        .submit(item("first", Priority::Low, 1), 0)
        .expect("the one skip the guard allows");
    assert!(first.started);
    assert!(matches!(
        dispatcher.submit(item("second", Priority::Low, 1), 0),
        Err(SchedulerError::QueueFull { depth: 1, .. })
    ));
    assert_eq!(dispatcher.skip_ahead_stats().skipped, 1);
    // An item that does not fit the free unit must wait: refused at depth.
    assert!(matches!(
        dispatcher.submit(item("wide", Priority::Critical, 2), 0),
        Err(SchedulerError::QueueFull { .. })
    ));
    // Strict order: nothing skips, so the small item is refused at depth.
    let strict = Dispatcher::in_memory("review", UnitSemaphore::new(4), 1).with_max_head_skips(0);
    let held = strict
        .submit(item("blocker", Priority::Normal, 2), 0)
        .expect("accepted");
    let _ = strict
        .submit(item("head", Priority::High, 4), 0)
        .expect("accepted");
    assert!(matches!(
        strict.submit(item("small", Priority::Low, 1), 0),
        Err(SchedulerError::QueueFull { .. })
    ));
    drop((blocker, first, held));
}

/// W3: a grant dropped instead of completed frees its units; the release
/// hook then pokes, and the parked item starts.
#[test]
fn review_w3_dropped_grant_wakes_parked_work_through_the_release_hook() {
    let dispatcher = Arc::new(Dispatcher::in_memory("review", UnitSemaphore::new(2), 8));
    let woken: Arc<Mutex<Vec<Grant<Item>>>> = Arc::default();
    let installed = dispatcher.install_release_hook(|| {
        let weak: Weak<Dispatcher<Item>> = Arc::downgrade(&dispatcher);
        let woken = Arc::clone(&woken);
        move || {
            if let Some(dispatcher) = weak.upgrade() {
                woken
                    .lock()
                    .expect("woken")
                    .extend(dispatcher.poke(0).started);
            }
        }
    });
    assert!(installed);
    assert!(dispatcher.has_release_hook());
    assert!(!dispatcher.install_release_hook(|| || unreachable!("installed once")));

    let running = dispatcher
        .submit(item("running", Priority::Normal, 2), 0)
        .expect("accepted");
    let parked = dispatcher
        .submit(item("parked", Priority::Normal, 1), 0)
        .expect("accepted");
    assert!(!parked.started);
    drop(running);
    assert_eq!(names(&woken.lock().expect("woken")), vec!["parked"]);
    assert_eq!(dispatcher.parked(), 0);
    assert_eq!(dispatcher.units_in_use(), 1);

    // `complete` drains itself and does not call the hook.
    let grant = woken.lock().expect("woken").pop().expect("parked started");
    let after = dispatcher.complete(grant.permit, 0);
    assert!(after.is_empty());
    assert!(woken.lock().expect("woken").is_empty());
    assert_eq!(dispatcher.units_in_use(), 0);
}

/// W3 without a hook: the documented obligation. The dropped grant frees its
/// units and the parked item waits until the owner pokes.
#[test]
fn review_w3_without_a_hook_the_owner_pokes_after_a_dropped_grant() {
    let dispatcher = Dispatcher::in_memory("review", UnitSemaphore::new(2), 8);
    let running = dispatcher
        .submit(item("running", Priority::Normal, 2), 0)
        .expect("accepted");
    let _ = dispatcher
        .submit(item("parked", Priority::Normal, 1), 0)
        .expect("accepted");
    drop(running);
    assert_eq!(dispatcher.units_in_use(), 0);
    assert_eq!(dispatcher.parked(), 1, "nothing drained: no hook installed");
    assert_eq!(names(&dispatcher.poke(0).started), vec!["parked"]);
}

/// S1: a cost whose per-kind units sum past `u32::MAX` is rejected, even on
/// a pool of `u32::MAX` units (its total saturates at `u32::MAX`).
#[test]
fn review_s1_cost_whose_sum_overflows_is_rejected() {
    let dispatcher = Dispatcher::in_memory("review", UnitSemaphore::new(u32::MAX), 8);
    let cost = ResourceCost::new(ResourceKind::Cpu, u32::MAX).with(ResourceKind::Io, 5);
    assert!(matches!(
        dispatcher.submit(costed("overflow", &cost), 0),
        Err(SchedulerError::CapacityExceeded { kind: None, .. })
    ));
    assert_eq!(dispatcher.units_in_use(), 0);
}

/// S2: a semaphore with no permits can never admit anything; submission says
/// so instead of parking work forever.
#[test]
fn review_s2_zero_permits_is_refused_with_a_clear_error() {
    let dispatcher = Dispatcher::in_memory("review", UnitSemaphore::with_max_permits(4, 0), 8);
    let refused = dispatcher.submit(item("never", Priority::Normal, 1), 0);
    assert!(
        matches!(&refused, Err(SchedulerError::InvalidConfig { .. })),
        "{refused:?}"
    );
    assert_eq!(dispatcher.parked(), 0);
}

/// A third-party queue that reports an expired head it then cannot pop.
struct VanishingHead {
    inner: PriorityQueue<Item>,
}

impl ParkedQueue<Item> for VanishingHead {
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn push(&mut self, item: Item) -> Result<(), SchedulerError> {
        self.inner.push(item)
    }

    fn head(&mut self) -> Result<Option<Ticket>, SchedulerError> {
        self.inner.head()
    }

    fn pop_head(&mut self) -> Option<Item> {
        None
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        self.inner.prune_expired(now_ms)
    }
}

/// S3: the drain stops, rather than spinning, when a queue's `pop_head`
/// returns nothing after `head` reported an expired head.
#[test]
fn review_s3_drain_stops_when_an_expired_head_cannot_be_popped() {
    let dispatcher = Arc::new(Dispatcher::new(
        "review",
        UnitSemaphore::new(1),
        8,
        VanishingHead {
            inner: PriorityQueue::new(),
        },
    ));
    let running = dispatcher
        .submit(item("running", Priority::Normal, 1), 0)
        .expect("accepted");
    let mut late = item("late", Priority::Normal, 1);
    late.deadline_ms = Some(5);
    let _ = dispatcher.submit(late, 0).expect("accepted");
    let (done, finished) = mpsc::channel();
    let poker = Arc::clone(&dispatcher);
    std::thread::spawn(move || {
        let woken = poker.poke(10);
        let _ = done.send(woken.is_empty());
    });
    let finished = finished.recv_timeout(Duration::from_secs(5));
    assert_eq!(finished, Ok(true), "the drain spun on the vanishing head");
    drop(running);
}

fn task(id: u64, deadline_ms: Option<u128>) -> ScheduledTask<u64> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: None,
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: 0,
        },
        payload: id,
    }
}

/// S5: `InMemoryQueue::prune_expired` uses the dispatcher's deadline rule
/// (`Ticket::is_expired`): a task is expired only once `now > deadline`.
#[test]
fn review_s5_prune_keeps_a_task_whose_deadline_is_now() {
    let mut queue = InMemoryQueue::new(8);
    queue.enqueue(task(1, Some(100))).expect("enqueue");
    queue.enqueue(task(2, Some(99))).expect("enqueue");
    let ticket = task(1, Some(100)).ticket();
    assert!(!ticket.is_expired(100), "live at its deadline");
    assert_eq!(queue.prune_expired(100).expect("prune"), 1, "only 99 < 100");
    assert_eq!(queue.len(), 1);
    assert_eq!(queue.prune_expired(101).expect("prune"), 1);
    assert_eq!(queue.len(), 0);
}

/// S6: a resource limit cannot be set once items have been submitted (here
/// one is parked and no permit is outstanding): a parked item over the new
/// limit could never start and would block the queue.
#[test]
fn review_s6_resource_limit_is_refused_while_items_are_parked() {
    let mut dispatcher = Dispatcher::in_memory("review", UnitSemaphore::new(16), 8);
    let vram = |units| ResourceCost::new(ResourceKind::GpuVram, units);
    let running = dispatcher
        .submit(costed("running", &vram(10)), 0)
        .expect("accepted");
    let parked = dispatcher
        .submit(costed("parked", &vram(8)), 0)
        .expect("accepted");
    assert!(!parked.started);
    // The permit goes back without a drain (no hook, no poke), so no permit
    // is outstanding while the 8-unit item is parked.
    drop(running);
    assert_eq!(dispatcher.running(), 0);
    assert!(matches!(
        dispatcher.set_resource_limit(ResourceKind::GpuVram, 4),
        Err(SchedulerError::InvalidConfig { .. })
    ));
    assert_eq!(dispatcher.resource_limit(ResourceKind::GpuVram), None);
    assert_eq!(names(&dispatcher.poke(0).started), vec!["parked"]);
}

/// A spawned future, held without being run.
type HeldFuture = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

/// Holds every spawned future without running it, so a dispatched
/// notification keeps the notifier's shared state alive.
#[derive(Clone, Default)]
struct HoldingSpawner {
    held: Arc<Mutex<Vec<HeldFuture>>>,
}

impl Spawn for HoldingSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.held.lock().expect("held").push(Box::pin(fut));
    }
}

#[derive(Clone, Copy)]
struct NeverSleeps;

impl Sleeper for NeverSleeps {
    fn sleep(&self, _duration: Duration) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}

#[derive(Clone, Copy)]
struct Silent;

impl Notifier<u32> for Silent {
    fn notify(
        &self,
        _key: &MailboxKey,
        _entry: &MailboxEntry<u32>,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        std::future::ready(Ok(()))
    }
}

/// S4: `with_policy` applies even after a notification was dispatched (the
/// in-flight one keeps its own policy).
#[test]
fn review_s4_with_policy_applies_after_a_dispatch() {
    let spawner = HoldingSpawner::default();
    let notifier = BackgroundNotifier::new(Silent, spawner.clone(), NeverSleeps);
    let key = MailboxKey {
        tenant: "t".into(),
        user_id: None,
        session_id: None,
    };
    DetachedNotifier::<u32>::dispatch(
        &notifier,
        &Delivery::new(key, 1, TaskStatus::Completed, Some(1_u32)),
    );
    assert_eq!(spawner.held.lock().expect("held").len(), 1);
    let policy = RetryPolicy::new(7, Duration::from_millis(1), Duration::from_millis(2));
    let notifier = notifier.with_policy(policy);
    let shown = format!("{notifier:?}");
    assert!(shown.contains("max_attempts: 7"), "{shown}");
}
