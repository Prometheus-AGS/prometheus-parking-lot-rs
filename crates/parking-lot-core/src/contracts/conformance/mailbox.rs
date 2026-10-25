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

/// A second delivery with the same address and remembered idempotency key
/// stores nothing even with another task id or payload; the first stands.
pub async fn first_write_wins<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let k = key("fww");
    let first = deliver(
        &mailbox,
        completed(&k, 1, "first").with_idempotency_key("idem"),
    )
    .await;
    let again = deliver(
        &mailbox,
        completed(&k, 2, "second").with_idempotency_key("idem"),
    )
    .await;
    assert_eq!(
        (first, again),
        (
            DeliveryOutcome::Stored,
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        )
    );

    assert_eq!(payload(consume(&mailbox, &k).await.as_ref()), Some("first"));
    assert!(consume(&mailbox, &k).await.is_none());
}

/// The same caller key is independent at addresses differing in tenant, user or session.
///
/// `None` and an empty optional identifier are different addresses. Every address
/// keeps its own first result and its tombstone after consume.
pub async fn idempotency_is_scoped_to_mailbox_key<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let base = key("idempotency-scope");
    let addresses = [
        base.clone(),
        key("idempotency-other-tenant"),
        MailboxKey {
            user_id: Some("other-user".into()),
            ..base.clone()
        },
        MailboxKey {
            session_id: Some("other-session".into()),
            ..base.clone()
        },
        MailboxKey {
            user_id: None,
            ..base.clone()
        },
        MailboxKey {
            user_id: Some(String::new()),
            ..base.clone()
        },
        MailboxKey {
            session_id: None,
            ..base.clone()
        },
        MailboxKey {
            session_id: Some(String::new()),
            ..base
        },
    ];
    for (address, payload) in addresses.iter().zip([
        "base",
        "other-tenant",
        "other-user",
        "other-session",
        "no-user",
        "empty-user",
        "no-session",
        "empty-session",
    ]) {
        assert_eq!(
            deliver(
                &mailbox,
                completed(address, 1, payload).with_idempotency_key("shared-key")
            )
            .await,
            DeliveryOutcome::Stored,
            "another address suppressed delivery to {address:?}"
        );
    }
    for (address, expected) in addresses.iter().zip([
        "base",
        "other-tenant",
        "other-user",
        "other-session",
        "no-user",
        "empty-user",
        "no-session",
        "empty-session",
    ]) {
        let retry = completed(address, 2, "replacement").with_idempotency_key("shared-key");
        assert_eq!(
            deliver(&mailbox, retry.clone()).await,
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
        assert_eq!(
            payload(consume(&mailbox, address).await.as_ref()),
            Some(expected)
        );
        assert_eq!(
            deliver(&mailbox, retry).await,
            DeliveryOutcome::Duplicate {
                status: TaskStatus::Completed
            }
        );
        assert!(consume(&mailbox, address).await.is_none());
    }
}

/// Expiry forgets only the original address's remembered key, including after
/// consume. An equal caller key at another address retains its own lifetime.
pub async fn idempotency_expiry_is_scoped_to_mailbox_key<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let a = key("scope-ttl-a");
    let b = key("scope-ttl-b");
    let expiring = completed(&a, 1, "a-first")
        .with_idempotency_key("same-caller-key")
        .with_ttl(h.ttl());
    let permanent = completed(&b, 2, "b-first").with_idempotency_key("same-caller-key");
    assert_eq!(
        deliver(&mailbox, expiring.clone()).await,
        DeliveryOutcome::Stored
    );
    assert_eq!(
        deliver(&mailbox, permanent.clone()).await,
        DeliveryOutcome::Stored
    );
    assert_eq!(
        payload(consume(&mailbox, &a).await.as_ref()),
        Some("a-first")
    );
    assert_eq!(
        deliver(&mailbox, expiring.clone()).await,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    h.advance(h.ttl()).await;
    assert_eq!(
        mailbox
            .evict_expired()
            .await
            .expect("evict consumed tombstone"),
        0
    );
    assert_eq!(deliver(&mailbox, expiring).await, DeliveryOutcome::Stored);
    assert_eq!(
        deliver(&mailbox, permanent.clone()).await,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    assert_eq!(
        payload(consume(&mailbox, &b).await.as_ref()),
        Some("b-first")
    );
    assert_eq!(
        deliver(&mailbox, permanent).await,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    assert!(consume(&mailbox, &b).await.is_none());
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
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    assert!(
        consume(&mailbox, &k).await.is_none(),
        "a consumed result was delivered again"
    );
}

/// A duplicate returns the first status without consuming the result.
///
/// This includes its full failure details and applies after consume. Current
/// statuses remain valid generic mailbox values; no terminal-only validation is
/// introduced.
pub async fn duplicate_returns_original_status<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let statuses = [
        TaskStatus::Failed(TaskError::transient("original gpu failure")),
        TaskStatus::Completed,
        TaskStatus::Expired,
        TaskStatus::Dropped("original drop reason".into()),
        TaskStatus::Queued,
        TaskStatus::Running,
    ];
    for (index, status) in statuses.into_iter().enumerate() {
        let address = key(&format!("original-status-{index}"));
        let first = Delivery::new(address.clone(), 1, status.clone(), Some("first".into()))
            .with_idempotency_key("same-caller");
        assert_eq!(deliver(&mailbox, first).await, DeliveryOutcome::Stored);
        let retried_status = if status == TaskStatus::Completed {
            TaskStatus::Failed(TaskError::executor("different retry failure"))
        } else {
            TaskStatus::Completed
        };
        let retry = Delivery::new(
            address.clone(),
            2,
            retried_status,
            Some("replacement".into()),
        )
        .with_idempotency_key("same-caller");
        let expected = DeliveryOutcome::Duplicate {
            status: status.clone(),
        };
        assert_eq!(
            deliver(&mailbox, retry.clone()).await,
            expected,
            "OA-07:stored-status oracle: duplicate reported the retry's status"
        );
        let visible = peek(&mailbox, &address)
            .await
            .expect("duplicate left result available");
        assert_eq!(visible.status, status);
        assert_eq!(visible.payload.as_deref(), Some("first"));
        let consumed = consume(&mailbox, &address)
            .await
            .expect("one original result");
        assert_eq!(consumed.status, status);
        assert_eq!(consumed.task_id, 1);
        assert_eq!(
            deliver(&mailbox, retry).await,
            expected,
            "OA-07:consumed-status oracle: consumption lost authoritative status"
        );
        assert!(consume(&mailbox, &address).await.is_none());
    }
}

/// Status metadata expires with its original fence. Reusing the key afterward
/// establishes a new first status, without retaining the previous generation.
pub async fn duplicate_status_expires_with_original_fence<H: MailboxHarness>(h: &H) {
    let mailbox = h.mailbox().await;
    let address = key("status-ttl");
    let failure = TaskStatus::Failed(TaskError::transient("first generation"));
    let original = Delivery::new(address.clone(), 1, failure.clone(), None)
        .with_idempotency_key("generation-key")
        .with_ttl(h.ttl());
    assert_eq!(deliver(&mailbox, original).await, DeliveryOutcome::Stored);
    assert!(consume(&mailbox, &address).await.is_some());
    let replacement =
        completed(&address, 2, "second generation").with_idempotency_key("generation-key");
    assert_eq!(
        deliver(&mailbox, replacement.clone()).await,
        DeliveryOutcome::Duplicate {
            status: failure.clone()
        }
    );
    h.advance(h.ttl()).await;
    assert_eq!(
        mailbox
            .evict_expired()
            .await
            .expect("expire consumed fence"),
        0
    );
    assert_eq!(
        deliver(&mailbox, replacement).await,
        DeliveryOutcome::Stored
    );
    let retry =
        Delivery::new(address.clone(), 3, failure, None).with_idempotency_key("generation-key");
    assert_eq!(
        deliver(&mailbox, retry).await,
        DeliveryOutcome::Duplicate {
            status: TaskStatus::Completed
        }
    );
    let result = consume(&mailbox, &address)
        .await
        .expect("second generation result");
    assert_eq!(result.task_id, 2);
    assert_eq!(result.status, TaskStatus::Completed);
    assert_eq!(result.payload.as_deref(), Some("second generation"));
    assert!(consume(&mailbox, &address).await.is_none());
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
    idempotency_is_scoped_to_mailbox_key(h).await;
    idempotency_expiry_is_scoped_to_mailbox_key(h).await;
    idempotency_survives_consume(h).await;
    duplicate_returns_original_status(h).await;
    duplicate_status_expires_with_original_fence(h).await;
    deliveries_without_key_all_stored(h).await;
    ttl_hides_and_evicts(h).await;
}
