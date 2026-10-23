//! Tests for the shared value types.

use prometheus_parking_lot_core::{MailboxKey, Priority, ResourceCost, ResourceKind, TaskId};

#[test]
fn priority_ordering() {
    assert!(Priority::Critical > Priority::High);
    assert!(Priority::High > Priority::Normal);
    assert!(Priority::Normal > Priority::Low);
}

#[test]
fn resource_cost_single_dimension() {
    let cost = ResourceCost::new(ResourceKind::GpuVram, 100);
    assert_eq!(cost.units_of(ResourceKind::GpuVram), 100);
    assert_eq!(cost.units_of(ResourceKind::Cpu), 0);
    assert_eq!(cost.total_units(), 100);
}

#[test]
fn resource_cost_multi_dimension() {
    let cost = ResourceCost::new(ResourceKind::GpuVram, 8).with(ResourceKind::Cpu, 1);
    assert_eq!(cost.dimensions(), 2);
    assert_eq!(cost.total_units(), 9);
}

#[test]
fn mailbox_key_fields() {
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
fn task_id_is_a_plain_integer() {
    let id: TaskId = 12345;
    assert_eq!(id, 12345);
}
