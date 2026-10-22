//! [`ResultMailbox`] conformance cases. Each takes a [`MailboxHarness`],
//! builds its own mailbox and panics on the first violated rule.

use super::MailboxHarness;
use crate::contracts::error::TaskError;
use crate::contracts::result_mailbox::{Delivery, DeliveryOutcome, MailboxEntry, ResultMailbox};
use crate::contracts::task::TaskStatus;
use crate::contracts::types::{MailboxKey, TaskId};

fn key(tenant: &str) -> MailboxKey {
    MailboxKey {
        tenant: tenant.to_owned(),
        user_id: Some("user".to_owned()),
        session_id: Some("session".to_owned()),
    }
}

fn completed(key: &MailboxKey, task_id: TaskId, payload: &str) -> Delivery<String> {
    Delivery::new(
        key.clone(),
        task_id,
        TaskStatus::Completed,
        Some(payload.to_owned()),
    )
}

async fn deliver<M: ResultMailbox<String>>(
    mailbox: &M,
    delivery: Delivery<String>,
) -> DeliveryOutcome {
    mailbox.deliver(delivery).await.expect("deliver")
}

async fn consume<M: ResultMailbox<String>>(
    mailbox: &M,
    key: &MailboxKey,
) -> Option<MailboxEntry<String>> {
    mailbox.consume(key).await.expect("consume")
}

async fn peek<M: ResultMailbox<String>>(
    mailbox: &M,
    key: &MailboxKey,
) -> Option<MailboxEntry<String>> {
    mailbox.peek(key).await.expect("peek")
}

fn payload(entry: Option<&MailboxEntry<String>>) -> Option<&str> {
    entry.and_then(|e| e.payload.as_deref())
}

/// A delivered entry comes back once, with its status (including a failure's
/// full `TaskError`), payload, task id and idempotency key; then it is gone.
pub async fn deliver_then_consume_once<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let k = key("consume-once");
    assert!(
        consume(&mailbox, &k).await.is_none(),
        "empty mailbox returned an entry"
    );

    let outcome = deliver(
        &mailbox,
        completed(&k, 1, "result").with_idempotency_key("idem-1"),
    )
    .await;
    assert_eq!(outcome, DeliveryOutcome::Stored);
    let failure = TaskStatus::Failed(TaskError::transient("gpu busy"));
    let _ = deliver(&mailbox, Delivery::new(k.clone(), 2, failure.clone(), None)).await;

    let first = consume(&mailbox, &k).await.expect("first entry");
    assert_eq!(first.task_id, 1);
    assert_eq!(first.status, TaskStatus::Completed);
    assert_eq!(first.payload.as_deref(), Some("result"));
    assert_eq!(first.idempotency_key.as_deref(), Some("idem-1"));

    let second = consume(&mailbox, &k).await.expect("second entry");
    assert_eq!(second.task_id, 2);
    assert_eq!(
        second.status, failure,
        "a failure's TaskError must round-trip"
    );
    assert_eq!(second.payload, None);

    assert!(
        consume(&mailbox, &k).await.is_none(),
        "an entry was consumed twice"
    );
}

/// Peek returns the oldest entry without removing it.
pub async fn peek_does_not_consume<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let k = key("peek");
    assert!(peek(&mailbox, &k).await.is_none());
    let _ = deliver(&mailbox, completed(&k, 1, "seen")).await;

    assert_eq!(payload(peek(&mailbox, &k).await.as_ref()), Some("seen"));
    assert_eq!(payload(peek(&mailbox, &k).await.as_ref()), Some("seen"));
    assert_eq!(payload(consume(&mailbox, &k).await.as_ref()), Some("seen"));
    assert!(peek(&mailbox, &k).await.is_none());
}

/// Entries for one key are consumed in delivery order.
pub async fn per_key_delivery_order<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let k = key("order");
    for (id, value) in [(1, "one"), (2, "two"), (3, "three")] {
        let _ = deliver(&mailbox, completed(&k, id, value)).await;
    }
    for expected in ["one", "two", "three"] {
        assert_eq!(
            payload(consume(&mailbox, &k).await.as_ref()),
            Some(expected)
        );
    }
}

/// Keys that differ in any field (tenant, user or session) are separate.
pub async fn keys_are_isolated<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let a = key("tenant-a");
    let b = key("tenant-b");
    let other_session = MailboxKey {
        session_id: Some("other".into()),
        ..a.clone()
    };
    let no_user = MailboxKey {
        user_id: None,
        ..a.clone()
    };
    let _ = deliver(&mailbox, completed(&a, 1, "for-a")).await;

    for other in [&b, &other_session, &no_user] {
        assert!(
            peek(&mailbox, other).await.is_none(),
            "{other:?} saw another key's entry"
        );
        assert!(consume(&mailbox, other).await.is_none());
    }
    assert_eq!(payload(consume(&mailbox, &a).await.as_ref()), Some("for-a"));
}

/// A second delivery with a remembered idempotency key stores nothing, even
/// under another mailbox key or with another payload; the first stands.
pub async fn first_write_wins<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let k = key("fww");
    let elsewhere = key("fww-elsewhere");
    let first = deliver(
        &mailbox,
        completed(&k, 1, "first").with_idempotency_key("idem"),
    )
    .await;
    let again = deliver(
        &mailbox,
        completed(&k, 1, "second").with_idempotency_key("idem"),
    )
    .await;
    let moved = deliver(
        &mailbox,
        completed(&elsewhere, 1, "third").with_idempotency_key("idem"),
    )
    .await;
    assert_eq!(
        (first, again, moved),
        (
            DeliveryOutcome::Stored,
            DeliveryOutcome::Duplicate,
            DeliveryOutcome::Duplicate
        )
    );

    assert_eq!(payload(consume(&mailbox, &k).await.as_ref()), Some("first"));
    assert!(consume(&mailbox, &k).await.is_none());
    assert!(consume(&mailbox, &elsewhere).await.is_none());
}

/// The idempotency key is still remembered after its entry is consumed, so a
/// redelivered task cannot hand the client a second result.
pub async fn idempotency_survives_consume<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let k = key("redelivery");
    let delivery = completed(&k, 1, "once").with_idempotency_key("idem-redelivered");
    assert_eq!(
        deliver(&mailbox, delivery.clone()).await,
        DeliveryOutcome::Stored
    );
    assert!(consume(&mailbox, &k).await.is_some());

    assert_eq!(
        deliver(&mailbox, delivery).await,
        DeliveryOutcome::Duplicate
    );
    assert!(
        consume(&mailbox, &k).await.is_none(),
        "a consumed result was delivered again"
    );
}

/// Deliveries without an idempotency key are all stored.
pub async fn deliveries_without_key_all_stored<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let k = key("unkeyed");
    assert_eq!(
        deliver(&mailbox, completed(&k, 1, "x")).await,
        DeliveryOutcome::Stored
    );
    assert_eq!(
        deliver(&mailbox, completed(&k, 1, "x")).await,
        DeliveryOutcome::Stored
    );
    assert!(consume(&mailbox, &k).await.is_some());
    assert!(consume(&mailbox, &k).await.is_some());
    assert!(consume(&mailbox, &k).await.is_none());
}

/// An entry past its TTL is never returned, eviction removes and counts it
/// and forgets its idempotency key; entries without a TTL stay.
pub async fn ttl_hides_and_evicts<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let ttl = h.ttl();
    let k = key("ttl");
    let untouched = key("ttl-untouched");
    let keeper = key("ttl-keeper");
    let expiring = completed(&k, 1, "short-lived")
        .with_idempotency_key("idem-ttl")
        .with_ttl(ttl);
    let _ = deliver(&mailbox, expiring.clone()).await;
    let _ = deliver(
        &mailbox,
        completed(&untouched, 2, "also short-lived").with_ttl(ttl),
    )
    .await;
    let _ = deliver(&mailbox, completed(&keeper, 3, "kept")).await;

    h.advance(ttl / 2).await;
    assert_eq!(
        payload(peek(&mailbox, &k).await.as_ref()),
        Some("short-lived"),
        "expired too early"
    );

    h.advance(ttl).await;
    assert!(
        peek(&mailbox, &k).await.is_none(),
        "peek returned an expired entry"
    );
    assert!(
        consume(&mailbox, &k).await.is_none(),
        "consume returned an expired entry"
    );
    // `untouched` was never read, so eviction must remove it; `k`'s entry may
    // already have been dropped lazily by the reads above.
    let evicted = mailbox.evict_expired().await.expect("evict");
    assert!(
        (1..=2).contains(&evicted),
        "evicted {evicted} entries, two expired"
    );
    assert_eq!(mailbox.evict_expired().await.expect("evict again"), 0);
    assert!(peek(&mailbox, &untouched).await.is_none());

    assert_eq!(
        payload(peek(&mailbox, &keeper).await.as_ref()),
        Some("kept")
    );
    assert_eq!(
        deliver(&mailbox, expiring).await,
        DeliveryOutcome::Stored,
        "an evicted entry's idempotency key is forgotten"
    );
}

/// Run every mailbox case in sequence, each on a fresh mailbox.
pub async fn run_all<H: MailboxHarness>(h: &H) {
    deliver_then_consume_once(h).await;
    peek_does_not_consume(h).await;
    per_key_delivery_order(h).await;
    keys_are_isolated(h).await;
    first_write_wins(h).await;
    idempotency_survives_consume(h).await;
    deliveries_without_key_all_stored(h).await;
    ttl_hides_and_evicts(h).await;
}
