//! Failure-injection tests for `StorageMailbox` (cr-34, scenario S-10).
//!
//! Real collaborators: the in-memory `ResultMailbox` as storage and a scripted
//! notifier behind `BackgroundNotifier`. Notification tasks run on real
//! threads (`ThreadSpawner`) driven by `futures::executor::block_on`, so the
//! core crate needs no async runtime to test off-path behaviour.
//!
//! Under `--cfg parking_lot_negative_control="s-10"` the mailbox notifies
//! before storing and aborts on a notifier error; the all-failing test then
//! fails because the delivery is lost.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use futures::channel::oneshot;
use futures::executor::block_on;
use parking_lot::Mutex;

use crate::contracts::result_mailbox::Notifier;
use crate::contracts::{
    Delivery, DeliveryOutcome, InMemoryResultMailbox, MailboxEntry, MailboxKey, ResultMailbox,
    SchedulerError, Spawn, StorageMailbox, TaskStatus,
};
use crate::mailbox::notify::{BackgroundNotifier, RetryPolicy, Sleeper};

#[derive(Clone, Default)]
struct ThreadSpawner {
    handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl ThreadSpawner {
    fn join_all(&self) {
        let handles: Vec<_> = self.handles.lock().drain(..).collect();
        for handle in handles {
            handle.join().expect("notification task panicked");
        }
    }
}

impl Spawn for ThreadSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        let handle = std::thread::spawn(move || block_on(fut));
        self.handles.lock().push(handle);
    }
}

/// Returns immediately and records the requested waits.
#[derive(Clone, Default)]
struct RecordingSleeper {
    waits: Arc<Mutex<Vec<Duration>>>,
}

impl Sleeper for RecordingSleeper {
    fn sleep(&self, duration: Duration) -> impl std::future::Future<Output = ()> + Send {
        self.waits.lock().push(duration);
        std::future::ready(())
    }
}

/// Fails its first `fail_first` calls, then succeeds. An optional gate holds
/// every call until the test releases it.
#[derive(Clone)]
struct ScriptedNotifier {
    fail_first: u32,
    calls: Arc<AtomicU32>,
    seen: Arc<Mutex<Vec<(MailboxKey, u64)>>>,
    gate: Arc<Mutex<Option<oneshot::Receiver<()>>>>,
}

impl ScriptedNotifier {
    fn failing(fail_first: u32) -> Self {
        Self {
            fail_first,
            calls: Arc::default(),
            seen: Arc::default(),
            gate: Arc::default(),
        }
    }

    fn always_failing() -> Self {
        Self::failing(u32::MAX)
    }

    fn healthy() -> Self {
        Self::failing(0)
    }

    fn gated() -> (Self, oneshot::Sender<()>) {
        let (tx, rx) = oneshot::channel();
        let notifier = Self::healthy();
        *notifier.gate.lock() = Some(rx);
        (notifier, tx)
    }

    fn calls(&self) -> u32 {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Notifier<u32> for ScriptedNotifier {
    fn notify(
        &self,
        key: &MailboxKey,
        entry: &MailboxEntry<u32>,
    ) -> impl std::future::Future<Output = Result<(), SchedulerError>> + Send {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let fail = call < self.fail_first;
        let gate = self.gate.lock().take();
        let record = (key.clone(), entry.task_id);
        let seen = Arc::clone(&self.seen);
        async move {
            if let Some(gate) = gate {
                let _ = gate.await;
            }
            if fail {
                Err(SchedulerError::backend(
                    "scripted notifier",
                    std::io::Error::other("down"),
                ))
            } else {
                seen.lock().push(record);
                Ok(())
            }
        }
    }
}

type Mailbox = StorageMailbox<
    InMemoryResultMailbox<u32>,
    BackgroundNotifier<ScriptedNotifier, ThreadSpawner, RecordingSleeper>,
>;

struct Rig {
    mailbox: Mailbox,
    notifier: ScriptedNotifier,
    spawner: ThreadSpawner,
    sleeper: RecordingSleeper,
}

fn rig(notifier: ScriptedNotifier, policy: RetryPolicy) -> Rig {
    let spawner = ThreadSpawner::default();
    let sleeper = RecordingSleeper::default();
    let background = BackgroundNotifier::new(notifier.clone(), spawner.clone(), sleeper.clone())
        .with_policy(policy);
    Rig {
        mailbox: StorageMailbox::new(InMemoryResultMailbox::new(), background),
        notifier,
        spawner,
        sleeper,
    }
}

fn policy(max_attempts: u32) -> RetryPolicy {
    RetryPolicy::new(
        max_attempts,
        Duration::from_millis(10),
        Duration::from_millis(30),
    )
}

fn key() -> MailboxKey {
    MailboxKey {
        tenant: "t".into(),
        user_id: Some("u".into()),
        session_id: None,
    }
}

fn delivery(task_id: u64) -> Delivery<u32> {
    Delivery::new(
        key(),
        task_id,
        TaskStatus::Completed,
        Some(u32::try_from(task_id).unwrap()),
    )
    .with_idempotency_key(format!("job-{task_id}"))
}

#[test]
fn storage_mailbox_notifier_failure_every_call_still_stores_and_consumes() {
    let rig = rig(ScriptedNotifier::always_failing(), policy(3));

    for id in 1..=10 {
        let outcome = block_on(rig.mailbox.deliver(delivery(id)))
            .expect("a failing notifier must not fail delivery");
        assert_eq!(outcome, DeliveryOutcome::Stored);
    }
    rig.spawner.join_all();

    let mut consumed = Vec::new();
    while let Some(entry) = block_on(rig.mailbox.consume(&key())).unwrap() {
        consumed.push(entry.task_id);
    }
    assert_eq!(
        consumed,
        (1..=10).collect::<Vec<_>>(),
        "every result is consumable, in order"
    );

    let stats = rig.mailbox.notifier().stats().snapshot();
    assert_eq!(
        (stats.notified, stats.retried, stats.abandoned),
        (0, 20, 10)
    );
    assert_eq!(rig.notifier.calls(), 30, "three attempts per notification");
}

#[test]
fn storage_mailbox_transient_failures_are_retried_with_backoff() {
    let rig = rig(ScriptedNotifier::failing(2), policy(5));

    let _ = block_on(rig.mailbox.deliver(delivery(7))).unwrap();
    rig.spawner.join_all();

    let stats = rig.mailbox.notifier().stats().snapshot();
    assert_eq!((stats.notified, stats.retried, stats.abandoned), (1, 2, 0));
    assert_eq!(rig.notifier.calls(), 3);
    assert_eq!(
        *rig.sleeper.waits.lock(),
        [Duration::from_millis(10), Duration::from_millis(20)]
    );
    assert_eq!(*rig.notifier.seen.lock(), [(key(), 7)]);
}

#[test]
fn storage_mailbox_persistent_failure_stops_at_the_bound_and_is_surfaced() {
    let rig = rig(ScriptedNotifier::failing(100), policy(4));

    let _ = block_on(rig.mailbox.deliver(delivery(1))).unwrap();
    rig.spawner.join_all();

    let stats = rig.mailbox.notifier().stats().snapshot();
    assert_eq!((stats.notified, stats.retried, stats.abandoned), (0, 3, 1));
    assert_eq!(rig.notifier.calls(), 4, "exactly max_attempts tries");
    assert_eq!(
        *rig.sleeper.waits.lock(),
        [
            Duration::from_millis(10),
            Duration::from_millis(20),
            Duration::from_millis(30)
        ],
        "backoff doubles and is capped"
    );
    assert!(block_on(rig.mailbox.peek(&key())).unwrap().is_some());
}

#[test]
fn storage_mailbox_slow_notifier_does_not_delay_deliver() {
    let (notifier, release) = ScriptedNotifier::gated();
    let rig = rig(notifier, policy(3));

    // The notifier blocks until released; deliver must return regardless.
    let outcome = block_on(rig.mailbox.deliver(delivery(1))).unwrap();
    assert_eq!(outcome, DeliveryOutcome::Stored);
    let entry = block_on(rig.mailbox.consume(&key()))
        .unwrap()
        .expect("stored while notifier is blocked");
    assert_eq!(entry.task_id, 1);
    assert_eq!(rig.mailbox.notifier().stats().snapshot().notified, 0);

    release.send(()).unwrap();
    rig.spawner.join_all();
    assert_eq!(rig.mailbox.notifier().stats().snapshot().notified, 1);
}

#[test]
fn storage_mailbox_duplicate_delivery_does_not_renotify() {
    let rig = rig(ScriptedNotifier::healthy(), policy(3));

    assert_eq!(
        block_on(rig.mailbox.deliver(delivery(5))).unwrap(),
        DeliveryOutcome::Stored
    );
    assert_eq!(
        block_on(rig.mailbox.deliver(delivery(5))).unwrap(),
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    rig.spawner.join_all();

    assert_eq!(rig.notifier.calls(), 1);
    assert_eq!(rig.mailbox.notifier().stats().snapshot().notified, 1);
}

/// Storage that always errors.
struct BrokenStorage;

impl ResultMailbox<u32> for BrokenStorage {
    fn deliver(
        &self,
        _delivery: Delivery<u32>,
    ) -> impl std::future::Future<Output = Result<DeliveryOutcome, SchedulerError>> + Send {
        std::future::ready(Err(SchedulerError::backend(
            "broken storage",
            std::io::Error::other("disk"),
        )))
    }

    fn consume(
        &self,
        _key: &MailboxKey,
    ) -> impl std::future::Future<Output = Result<Option<MailboxEntry<u32>>, SchedulerError>> + Send
    {
        std::future::ready(Ok(None))
    }

    fn peek(
        &self,
        _key: &MailboxKey,
    ) -> impl std::future::Future<Output = Result<Option<MailboxEntry<u32>>, SchedulerError>> + Send
    {
        std::future::ready(Ok(None))
    }

    fn evict_expired(
        &self,
    ) -> impl std::future::Future<Output = Result<usize, SchedulerError>> + Send {
        std::future::ready(Ok(0))
    }
}

#[test]
fn storage_mailbox_failed_store_is_reported_and_not_notified() {
    let notifier = ScriptedNotifier::healthy();
    let spawner = ThreadSpawner::default();
    let mailbox = StorageMailbox::new(
        BrokenStorage,
        BackgroundNotifier::new(
            notifier.clone(),
            spawner.clone(),
            RecordingSleeper::default(),
        ),
    );

    assert!(block_on(mailbox.deliver(delivery(1))).is_err());
    spawner.join_all();
    assert_eq!(notifier.calls(), 0, "nothing stored, nothing announced");
}

#[test]
fn storage_mailbox_storage_only_has_no_notifier_cost() {
    let mailbox = StorageMailbox::storage_only(InMemoryResultMailbox::<u32>::new());
    assert_eq!(
        block_on(mailbox.deliver(delivery(1))).unwrap(),
        DeliveryOutcome::Stored
    );
    assert_eq!(
        block_on(mailbox.consume(&key())).unwrap().unwrap().task_id,
        1
    );
}

#[test]
fn storage_mailbox_retry_policy_is_a_pure_capped_doubling() {
    let p = RetryPolicy::new(5, Duration::from_millis(100), Duration::from_millis(350));
    let waits: Vec<_> = (1..=5).map(|a| p.backoff_after(a)).collect();
    assert_eq!(
        waits,
        [
            Some(Duration::from_millis(100)),
            Some(Duration::from_millis(200)),
            Some(Duration::from_millis(350)),
            Some(Duration::from_millis(350)),
            None
        ]
    );
    assert_eq!(
        RetryPolicy::new(0, Duration::ZERO, Duration::ZERO).backoff_after(1),
        None
    );
    assert_eq!(p.backoff_after(u32::MAX), None);
}
