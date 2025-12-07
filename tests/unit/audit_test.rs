//! Tests for audit sink

use prometheus_parking_lot::core::{AuditSink, InMemoryAuditSink, build_audit_event};

#[test]
fn test_in_memory_audit_sink() {
    let mut sink = InMemoryAuditSink::new(10);
    
    let event = build_audit_event(
        "evt1",
        "task1",
        "pool1",
        "tenant1",
        "submit",
        Some("payload".to_string()),
    );
    
    sink.record(event.clone());
    assert_eq!(sink.events().len(), 1);
    
    let events = sink.events();
    assert_eq!(events[0].event_id, "evt1");
    assert_eq!(events[0].task_id, "task1");
    assert_eq!(events[0].action, "submit");
}

#[test]
fn test_audit_sink_overflow() {
    let mut sink = InMemoryAuditSink::new(2);
    
    sink.record(build_audit_event("evt1", "task1", "pool1", "tenant1", "submit", None));
    sink.record(build_audit_event("evt2", "task2", "pool1", "tenant1", "submit", None));
    sink.record(build_audit_event("evt3", "task3", "pool1", "tenant1", "submit", None));
    
    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_id, "evt2"); // First one popped
    assert_eq!(events[1].event_id, "evt3");
}

#[test]
fn test_build_audit_event() {
    let event = build_audit_event(
        "evt1",
        "task1",
        "pool1",
        "tenant1",
        "complete",
        Some("result".to_string()),
    );
    
    assert_eq!(event.event_id, "evt1");
    assert_eq!(event.task_id, "task1");
    assert_eq!(event.pool, "pool1");
    assert_eq!(event.tenant, "tenant1");
    assert_eq!(event.action, "complete");
    assert_eq!(event.payload, Some("result".to_string()));
    assert!(event.created_at_ms > 0);
}
