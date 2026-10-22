//! Integration tests for PostgresMailbox implementation.

use prometheus_parking_lot_core::{Mailbox, MailboxKey, TaskError, TaskStatus, now_ms};
use prometheus_parking_lot_pg::PostgresMailbox;

use super::{PostgresTestFixture, TestPayload, blocking};

fn make_key(tenant: &str, user_id: Option<&str>, session_id: Option<&str>) -> MailboxKey {
    MailboxKey {
        tenant: tenant.into(),
        user_id: user_id.map(String::from),
        session_id: session_id.map(String::from),
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_deliver_and_fetch() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());
    let key = make_key("tenant1", Some("user1"), None);

    // Deliver a message
    let payload = TestPayload::new(1, "result-1");
    blocking!(mailbox
        .deliver(&key, TaskStatus::Completed, Some(payload.clone())))
        .expect("deliver failed");

    // Fetch messages
    let messages = mailbox
        .fetch(&key, None, 10)
        .await
        .expect("fetch failed");

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].payload.as_ref().unwrap(), &payload);
    assert_eq!(messages[0].status, "Completed");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_fetch_by_key() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());

    // Deliver messages for different tenants/users
    let key1 = make_key("tenant1", Some("user1"), None);
    let key2 = make_key("tenant1", Some("user2"), None);
    let key3 = make_key("tenant2", Some("user1"), None);

    blocking!(mailbox.deliver(&key1, TaskStatus::Completed, Some(TestPayload::new(1, "t1-u1")))).expect("deliver failed");
    blocking!(mailbox.deliver(&key2, TaskStatus::Completed, Some(TestPayload::new(2, "t1-u2")))).expect("deliver failed");
    blocking!(mailbox.deliver(&key3, TaskStatus::Completed, Some(TestPayload::new(3, "t2-u1")))).expect("deliver failed");

    // Fetch for key1 should only return its messages
    let messages1 = mailbox.fetch(&key1, None, 10).await.expect("fetch failed");
    assert_eq!(messages1.len(), 1);
    assert_eq!(messages1[0].payload.as_ref().unwrap().id, 1);

    // Fetch for key2
    let messages2 = mailbox.fetch(&key2, None, 10).await.expect("fetch failed");
    assert_eq!(messages2.len(), 1);
    assert_eq!(messages2[0].payload.as_ref().unwrap().id, 2);

    // Fetch for key3
    let messages3 = mailbox.fetch(&key3, None, 10).await.expect("fetch failed");
    assert_eq!(messages3.len(), 1);
    assert_eq!(messages3[0].payload.as_ref().unwrap().id, 3);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_fetch_since_timestamp() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());
    let key = make_key("tenant1", Some("user1"), None);

    // Deliver messages at different times
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(1, "msg1")))).expect("deliver failed");
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    
    let timestamp_between = now_ms();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(2, "msg2")))).expect("deliver failed");
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(3, "msg3")))).expect("deliver failed");

    // Fetch all messages
    let all_messages = mailbox.fetch(&key, None, 10).await.expect("fetch failed");
    assert_eq!(all_messages.len(), 3);

    // Fetch only messages after timestamp
    let recent_messages = mailbox.fetch(&key, Some(timestamp_between), 10).await.expect("fetch failed");
    assert_eq!(recent_messages.len(), 2);
    assert!(recent_messages.iter().all(|m| m.payload.as_ref().unwrap().id >= 2));
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_multiple_messages_ordering() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());
    let key = make_key("tenant1", Some("user1"), None);

    // Deliver multiple messages
    for i in 1..=5 {
        blocking!(mailbox
            .deliver(&key, TaskStatus::Completed, Some(TestPayload::new(i, format!("msg{}", i)))))
            .expect("deliver failed");
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    // Fetch should return in chronological order
    let messages = mailbox.fetch(&key, None, 10).await.expect("fetch failed");
    assert_eq!(messages.len(), 5);
    
    for (idx, msg) in messages.iter().enumerate() {
        assert_eq!(msg.payload.as_ref().unwrap().id, (idx + 1) as u64);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_fetch_with_limit() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());
    let key = make_key("tenant1", Some("user1"), None);

    // Deliver 10 messages
    for i in 1..=10 {
        blocking!(mailbox
            .deliver(&key, TaskStatus::Completed, Some(TestPayload::new(i, format!("msg{}", i)))))
            .expect("deliver failed");
    }

    // Fetch with limit
    let messages = mailbox.fetch(&key, None, 3).await.expect("fetch failed");
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0].payload.as_ref().unwrap().id, 1);
    assert_eq!(messages[1].payload.as_ref().unwrap().id, 2);
    assert_eq!(messages[2].payload.as_ref().unwrap().id, 3);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_prune_old_messages() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());
    let key = make_key("tenant1", Some("user1"), None);

    // Deliver old messages
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(1, "old1")))).expect("deliver failed");
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(2, "old2")))).expect("deliver failed");
    
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    let cutoff_time = now_ms();
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    // Deliver new messages
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(3, "new1")))).expect("deliver failed");
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(4, "new2")))).expect("deliver failed");

    // Prune old messages
    let pruned = mailbox.prune_old(cutoff_time).await.expect("prune failed");
    assert_eq!(pruned, 2);

    // Only new messages should remain
    let messages = mailbox.fetch(&key, None, 10).await.expect("fetch failed");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].payload.as_ref().unwrap().id, 3);
    assert_eq!(messages[1].payload.as_ref().unwrap().id, 4);
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_different_statuses() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());
    let key = make_key("tenant1", Some("user1"), None);

    // Deliver messages with different statuses
    blocking!(mailbox.deliver(&key, TaskStatus::Queued, None::<TestPayload>)).expect("deliver failed");
    blocking!(mailbox.deliver(&key, TaskStatus::Running, None::<TestPayload>)).expect("deliver failed");
    blocking!(mailbox.deliver(&key, TaskStatus::Completed, Some(TestPayload::new(1, "result")))).expect("deliver failed");
    blocking!(mailbox.deliver(&key, TaskStatus::Failed(TaskError::executor("error")), None::<TestPayload>)).expect("deliver failed");
    blocking!(mailbox.deliver(&key, TaskStatus::Expired, None::<TestPayload>)).expect("deliver failed");

    let messages = mailbox.fetch(&key, None, 10).await.expect("fetch failed");
    assert_eq!(messages.len(), 5);

    assert_eq!(messages[0].status, "Queued");
    assert_eq!(messages[1].status, "Running");
    assert_eq!(messages[2].status, "Completed");
    assert!(messages[3].status.starts_with("Failed:"));
    assert_eq!(messages[4].status, "Expired");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "needs Docker: starts a parking-lot-pg container; run with `cargo test -p prometheus-parking-lot-pg -- --ignored`"]
async fn test_postgres_mailbox_session_isolation() {
    let fixture = PostgresTestFixture::new()
        .await
        .expect("Failed to create test fixture");

    let mut mailbox = PostgresMailbox::new(fixture.db.clone());

    // Same tenant and user, different sessions
    let key1 = make_key("tenant1", Some("user1"), Some("session1"));
    let key2 = make_key("tenant1", Some("user1"), Some("session2"));

    blocking!(mailbox.deliver(&key1, TaskStatus::Completed, Some(TestPayload::new(1, "s1")))).expect("deliver failed");
    blocking!(mailbox.deliver(&key2, TaskStatus::Completed, Some(TestPayload::new(2, "s2")))).expect("deliver failed");

    // Each session should only see its own messages
    let messages1 = mailbox.fetch(&key1, None, 10).await.expect("fetch failed");
    assert_eq!(messages1.len(), 1);
    assert_eq!(messages1[0].payload.as_ref().unwrap().id, 1);

    let messages2 = mailbox.fetch(&key2, None, 10).await.expect("fetch failed");
    assert_eq!(messages2.len(), 1);
    assert_eq!(messages2[0].payload.as_ref().unwrap().id, 2);
}
