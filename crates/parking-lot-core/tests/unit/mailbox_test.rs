//! Tests for the in-memory mailbox.

use prometheus_parking_lot_core::{InMemoryMailbox, Mailbox, MailboxKey, TaskStatus};

fn make_key(session: &str) -> MailboxKey {
    MailboxKey {
        tenant: "tenant1".to_string(),
        user_id: None,
        session_id: Some(session.to_string()),
    }
}

#[test]
fn deliver_then_consume_returns_the_payload_once() {
    let mut mailbox = InMemoryMailbox::<String>::new();
    let key = make_key("session1");

    mailbox
        .deliver(&key, TaskStatus::Completed, Some("result".to_string()))
        .expect("deliver");

    let (status, payload) = mailbox
        .consume(&key)
        .expect("consume")
        .expect("message present");
    assert_eq!(status, TaskStatus::Completed);
    assert_eq!(payload, Some("result".to_string()));

    // Consume-once: a second consume finds nothing.
    assert!(mailbox.consume(&key).expect("consume").is_none());
}

#[test]
fn later_delivery_overwrites_the_status() {
    let mut mailbox = InMemoryMailbox::<String>::new();
    let key = make_key("session2");

    mailbox
        .deliver(&key, TaskStatus::Queued, None)
        .expect("deliver queued");
    assert_eq!(mailbox.peek(&key).map(|m| m.status.clone()), Some(TaskStatus::Queued));

    mailbox
        .deliver(&key, TaskStatus::Completed, Some("done".to_string()))
        .expect("deliver completed");
    assert_eq!(mailbox.len(), 1);
    assert_eq!(
        mailbox.peek(&key).map(|m| m.status.clone()),
        Some(TaskStatus::Completed)
    );
}

#[test]
fn expired_messages_are_evicted() {
    let mut mailbox = InMemoryMailbox::<String>::new();
    let key = make_key("session3");

    mailbox
        .deliver_with_ttl(&key, TaskStatus::Completed, None, 1)
        .expect("deliver with ttl");
    let evicted = mailbox
        .evict_expired(u128::MAX)
        .expect("evict");
    assert_eq!(evicted, 1);
    assert!(mailbox.is_empty());
}
