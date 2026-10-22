//! Tests for utility functions

use prometheus_parking_lot::util::{Priority, ResourceCost, ResourceKind, TaskId, MailboxKey};

#[test]
fn test_priority_ordering() {
    assert!(Priority::Critical > Priority::High);
    assert!(Priority::High > Priority::Normal);
    assert!(Priority::Normal > Priority::Low);
}

#[test]
fn test_resource_cost() {
    let cost = ResourceCost::new(ResourceKind::GpuVram, 100);
    assert_eq!(cost.units_of(ResourceKind::GpuVram), 100);
    assert_eq!(cost.total_units(), 100);
}

#[test]
fn test_mailbox_key() {
    let key = MailboxKey {
        tenant: "tenant1".to_string(),
        user_id: Some("user1".to_string()),
        session_id: Some("session1".to_string()),
    };
    assert_eq!(key.tenant, "tenant1");
    assert_eq!(key.user_id, Some("user1".to_string()));
    assert_eq!(key.session_id, Some("session1".to_string()));
}

#[test]
fn test_task_id() {
    let id: TaskId = 12345;
    assert_eq!(id, 12345);
}
