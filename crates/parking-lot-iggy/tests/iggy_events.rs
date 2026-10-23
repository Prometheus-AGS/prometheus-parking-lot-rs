//! Scenario S-11 (cr-32): lifecycle events and `mailbox.ready` notifications
//! reach a pinned Apache Iggy server with deterministic ids, and a server that
//! stops answering never slows the dispatch path.
//!
//! Every test starts an `apache/iggy` container, so each is `#[ignore]`d and
//! compiles in CI without Docker. Run them with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-iggy --locked --run-ignored all -E "test(iggy_)"
//! ```
//!
//! With `--cfg parking_lot_negative_control="s-11"` the ids are random and the
//! publish is synchronous; the id and latency tests below must then fail.

mod support;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use prometheus_parking_lot_core::mailbox::{BackgroundNotifier, RetryPolicy};
use prometheus_parking_lot_core::{
    Delivery, HookBus, InMemoryResultMailbox, LifecycleEvent, MailboxKey, ResultMailbox,
    StorageMailbox, TaskStatus, emit_completed, emit_submitted,
};
use prometheus_parking_lot_iggy::ids::mailbox_ready_id;
use prometheus_parking_lot_iggy::{
    IggyHook, IggyNotifier, IggyOptions, TokioHandleSpawner, TokioSleeper,
};
use support::{IggyServer, TestResult, read_topic, wait_for_distinct_messages, wait_for_messages};

const PARTITIONS: u32 = 4;
const FLUSH: Duration = Duration::from_secs(30);

fn options(server: &IggyServer, stream: &str) -> IggyOptions {
    IggyOptions::new(server.connection_string.clone())
        .with_stream(stream)
        .with_partitions(PARTITIONS)
}

fn mailbox_key(tenant: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.to_owned(),
        user_id: None,
        session_id: Some("s1".to_owned()),
    }
}

fn ready_ids(messages: &[support::Read]) -> Vec<u128> {
    messages.iter().map(|m| m.id).collect()
}

/// A subscriber receives `mailbox.ready` for each task completed through a
/// `StorageMailbox` over a real `BackgroundNotifier<IggyNotifier>`, and can
/// replay the partition from an earlier offset.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_subscriber_receives_mailbox_ready_and_replays_from_an_offset() -> TestResult<()> {
    let server = IggyServer::start().await?;
    let notifier = IggyNotifier::start(&options(&server, "ready"))?;
    let mailbox = StorageMailbox::new(
        InMemoryResultMailbox::<String>::new(),
        BackgroundNotifier::new(
            notifier.clone(),
            TokioHandleSpawner::current(),
            TokioSleeper,
        ),
    );

    let key = mailbox_key("acme");
    let task_ids: Vec<u64> = (1..=6).collect();
    for task_id in &task_ids {
        let delivery = Delivery::new(
            key.clone(),
            *task_id,
            TaskStatus::Completed,
            Some(format!("result {task_id}")),
        )
        .with_idempotency_key(format!("job-{task_id}"));
        let outcome = mailbox.deliver(delivery).await?;
        assert_eq!(
            outcome,
            prometheus_parking_lot_core::DeliveryOutcome::Stored
        );
    }
    // The notifications are off-path: the results are stored before any is sent.
    assert!(mailbox.peek(&key).await?.is_some(), "result stored");
    wait_published(&notifier, task_ids.len()).await;
    assert!(notifier.flush(FLUSH).await, "publisher drained");

    let client = server.client().await?;
    // At-least-once: a send that reached the server but timed out is retried
    // and stored twice under one id, so assert on distinct ids.
    let messages =
        wait_for_distinct_messages(&client, "ready", "mailbox.ready", PARTITIONS, 6).await?;
    assert_eq!(messages.len(), 6, "one mailbox.ready per completed task");

    let seen: BTreeSet<u64> = messages
        .iter()
        .map(|m| m.body["task_id"].as_u64().unwrap_or(0))
        .collect();
    assert_eq!(seen, task_ids.iter().copied().collect::<BTreeSet<_>>());
    for message in &messages {
        assert_eq!(message.body["event"], "mailbox.ready");
        assert_eq!(message.body["mailbox_key"]["tenant"], "acme");
        assert_eq!(message.body["status"], "Completed");
        assert_eq!(message.body["result_ref"], "acme/s1");
        let job = message.body["idempotency_key"].as_str().unwrap_or_default();
        assert_eq!(
            message.id,
            mailbox_ready_id(job),
            "id derives from the idempotency key"
        );
    }
    let partitions: BTreeSet<u32> = messages.iter().map(|m| m.partition).collect();
    assert_eq!(
        partitions.len(),
        1,
        "one tenant, one partition (ordering per tenant)"
    );

    // Replay: read the partition again, then only the tail from an offset.
    // The offset is that of the third distinct message, so the tail must be the
    // later four distinct ids in order; an earlier id's retried copy that lands
    // after that offset is not a new message.
    let partition = messages[0].partition;
    let in_partition = |all: Vec<support::Read>| -> Vec<support::Read> {
        all.into_iter()
            .filter(|m| m.partition == partition)
            .collect()
    };
    let from_start = support::distinct_by_id(in_partition(
        read_topic(&client, "ready", "mailbox.ready", PARTITIONS, 0).await?,
    ));
    assert_eq!(
        ready_ids(&from_start),
        ready_ids(&messages),
        "replay from 0 yields the same distinct ids in the same order"
    );
    let from = from_start[2].offset;
    let earlier: BTreeSet<u128> = from_start[..2].iter().map(|m| m.id).collect();
    let tail: Vec<_> = support::distinct_by_id(in_partition(
        read_topic(&client, "ready", "mailbox.ready", PARTITIONS, from).await?,
    ))
    .into_iter()
    .filter(|m| !earlier.contains(&m.id))
    .collect();
    assert_eq!(
        ready_ids(&tail),
        ready_ids(&from_start[2..]),
        "replay from offset {from} yields the later distinct ids in order"
    );
    assert_eq!(tail[0].offset, from);
    Ok(())
}

/// The same idempotency key published twice, by the same notifier or after a
/// restart, carries one message id; a different key carries another.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_same_idempotency_key_yields_one_message_id() -> TestResult<()> {
    use prometheus_parking_lot_core::{MailboxEntry, Notifier};

    let server = IggyServer::start().await?;
    let notifier = IggyNotifier::start(&options(&server, "ids"))?;
    let key = mailbox_key("acme");
    let entry = |task_id: u64, idem: &str| {
        MailboxEntry::new(
            task_id,
            TaskStatus::Completed,
            Some(1_u8),
            Some(idem.to_owned()),
            7,
        )
    };

    // Twice from one notifier (a retried notification) ...
    notifier.notify(&key, &entry(1, "order-42")).await?;
    notifier.notify(&key, &entry(1, "order-42")).await?;
    // ... and once after a restart: a fresh notifier has no memory of the first.
    let restarted = IggyNotifier::start(&options(&server, "ids"))?;
    restarted.notify(&key, &entry(1, "order-42")).await?;
    notifier.notify(&key, &entry(2, "order-43")).await?;
    assert!(notifier.flush(FLUSH).await && restarted.flush(FLUSH).await);

    let client = server.client().await?;
    let messages = wait_for_messages(&client, "ids", "mailbox.ready", PARTITIONS, 3).await?;
    let for_order = |idem: &str| -> Vec<u128> {
        messages
            .iter()
            .filter(|m| m.body["idempotency_key"] == idem)
            .map(|m| m.id)
            .collect()
    };
    let first = for_order("order-42");
    let distinct: BTreeSet<u128> = first.iter().copied().collect();
    assert_eq!(
        distinct.len(),
        1,
        "one message id for one idempotency key: {first:?}"
    );
    assert_eq!(
        distinct.iter().next().copied(),
        Some(mailbox_ready_id("order-42"))
    );
    let other = for_order("order-43");
    assert_eq!(other, vec![mailbox_ready_id("order-43")]);
    assert_ne!(first[0], other[0], "different keys, different ids");
    // The in-process window drops the repeat; the restarted notifier's copy is
    // the consumer's to de-duplicate by id.
    assert_eq!(notifier.stats().deduplicated, 1);
    assert_eq!(
        first.len(),
        2,
        "one per publisher instance, same id: {first:?}"
    );
    Ok(())
}

/// Lifecycle events from a `HookBus` arrive on the `lifecycle` topic in
/// emission order, partitioned by pool when no tenant is configured.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_lifecycle_events_arrive_in_order_for_a_pool() -> TestResult<()> {
    let server = IggyServer::start().await?;
    let hook = IggyHook::start(&options(&server, "lifecycle"))?;
    let bus = HookBus::new();
    bus.register(Arc::new(hook.clone()));
    for task_id in 1..=3_u64 {
        emit_submitted(&bus, task_id, Some(format!("job-{task_id}")), "gpu");
        emit_completed(&bus, task_id, "gpu", 5);
    }
    flush_bus(&bus, Duration::from_secs(10)).await?;
    assert!(hook.flush(FLUSH).await);

    let client = server.client().await?;
    // At-least-once: a send that reached the server but timed out is retried
    // and stored twice under one id (seen under heavy load), so compare the
    // distinct ids, in order.
    let messages =
        wait_for_distinct_messages(&client, "lifecycle", "lifecycle", PARTITIONS, 6).await?;
    assert_eq!(messages.len(), 6, "six distinct lifecycle events");
    let partitions: BTreeSet<u32> = messages.iter().map(|m| m.partition).collect();
    assert_eq!(partitions.len(), 1, "one pool, one partition");
    let events: Vec<String> = messages
        .iter()
        .map(|m| {
            format!(
                "{}:{}",
                m.body["event"].as_str().unwrap_or(""),
                m.body["task_id"]
            )
        })
        .collect();
    assert_eq!(
        events,
        [
            "submitted:1",
            "completed:1",
            "submitted:2",
            "completed:2",
            "submitted:3",
            "completed:3"
        ]
    );
    assert_eq!(messages[0].body["idempotency_key"], "job-1");
    Ok(())
}

/// With the server frozen (`docker pause`) the hook adds no dispatch latency:
/// `on_event` stays within a bound, the bus delivers every event without a drop,
/// and the surplus is dropped by the publisher and counted. After the server
/// resumes, publishing recovers.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_paused_server_does_not_slow_dispatch() -> TestResult<()> {
    const EVENTS: usize = 2000;
    const QUEUE: usize = 64;
    /// Bound on one `on_event` call and on the whole burst through the bus.
    const ON_EVENT_BOUND: Duration = Duration::from_millis(50);
    const BURST_BOUND: Duration = Duration::from_secs(2);

    let server = IggyServer::start().await?;
    let options = options(&server, "paused")
        .with_queue_capacity(QUEUE)
        .with_send_timeout(Duration::from_millis(400))
        .with_retry(RetryPolicy::new(
            2,
            Duration::from_millis(50),
            Duration::from_millis(100),
        ));
    let hook = IggyHook::start(&options)?;
    let bus = HookBus::new();
    bus.register(Arc::new(hook.clone()));

    // Warm: the first event creates the stream and topic and proves the path.
    emit_submitted(&bus, 0, None, "gpu");
    flush_bus(&bus, FLUSH).await?;
    assert!(hook.flush(FLUSH).await);
    let baseline = hook.stats();
    assert_eq!(baseline.published, 1);

    server.pause()?;
    let probe = LifecycleEvent::Failed {
        task_id: 1,
        pool: "gpu".to_owned(),
        error: "probe".to_owned(),
        at_ms: 1,
    };
    let started = Instant::now();
    prometheus_parking_lot_core::Hook::on_event(&hook, &probe);
    let single = started.elapsed();

    let started = Instant::now();
    for task_id in 2..EVENTS as u64 {
        emit_completed(&bus, task_id, "gpu", 1);
    }
    let emit_time = started.elapsed();
    let delivered_in_time = flush_bus(&bus, BURST_BOUND).await.is_ok();
    let burst = started.elapsed();
    let stats = hook.stats();
    server.unpause()?;

    assert!(
        single < ON_EVENT_BOUND,
        "on_event took {single:?} with the server paused"
    );
    assert!(emit_time < BURST_BOUND, "emitting took {emit_time:?}");
    assert!(
        delivered_in_time,
        "the bus delivery thread was stalled by the hook ({burst:?})"
    );
    assert_eq!(bus.dropped_events(), 0, "the hook never backed the bus up");
    assert!(
        stats.dropped > 0,
        "the surplus is dropped and counted: {stats:?}"
    );
    let accepted = stats.enqueued - baseline.enqueued;
    let dropped = stats.dropped - baseline.dropped;
    assert_eq!(
        accepted + dropped,
        (EVENTS - 1) as u64,
        "every event is queued or counted"
    );
    assert!(
        accepted <= (QUEUE + 2) as u64,
        "buffering is bounded: {accepted}"
    );

    // The server is back: the buffered events are published.
    assert!(
        hook.flush(FLUSH).await,
        "publisher drains after the server resumes"
    );
    assert!(
        hook.stats().published > baseline.published,
        "publishing recovered"
    );
    Ok(())
}

/// Wait until the bus has handed every emitted event to its hooks.
async fn flush_bus(bus: &HookBus, timeout: Duration) -> TestResult<()> {
    let bus = bus.clone();
    if tokio::task::spawn_blocking(move || bus.flush(timeout)).await? {
        Ok(())
    } else {
        Err("the hook bus did not deliver within the timeout".into())
    }
}

/// Wait until `notifier` has published `count` notifications.
async fn wait_published(notifier: &IggyNotifier, count: usize) {
    let deadline = Instant::now() + FLUSH;
    while notifier.stats().published < count as u64 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
