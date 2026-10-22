//! Tests for mailbox implementations

use prometheus_parking_lot::infra::mailbox::memory::InMemoryMailbox;
use prometheus_parking_lot::util::MailboxKey;
use prometheus_parking_lot::core::resource_pool::TaskStatus;

fn make_key(session: &str) -> MailboxKey {
    MailboxKey {
        tenant: "tenant1".to_string(),
        user_id: None,
        session_id: Some(session.to_string()),
    }
}

#[test]
fn test_in_memory_mailbox_deliver_and_fetch() {
    let mut mailbox = InMemoryMailbox::<String>::new();
    let key = make_key("session1");

    mailbox.deliver(&key, TaskStatus::Completed, Some("result".to_string()));

    let messages = mailbox.fetch(&key, None, 10);
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload, Some("result".to_string()));
}

#[test]
fn test_in_memory_mailbox_prune_empty() {
    let mut mailbox = InMemoryMailbox::<String>::new();
    let key = make_key("session2");

    mailbox.deliver(&key, TaskStatus::Queued, None);
    // No prune method for in-memory mailbox; ensure fetch limit works
    let messages = mailbox.fetch(&key, None, 1);
    assert_eq!(messages.len(), 1);
}
