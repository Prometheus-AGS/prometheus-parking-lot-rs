//! Tests for the audit sink.

use prometheus_parking_lot_core::{AuditSink, InMemoryAuditSink, build_audit_event};

#[test]
fn in_memory_audit_sink_records_events() {
    let mut sink = InMemoryAuditSink::new(10);

    let event = build_audit_event(
        "evt1",
        "task1",
        "pool1",
        "tenant1",
        "submit",
        Some("payload".to_string()),
    );

    sink.record(event);

    let events = sink.events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_id, "evt1");
    assert_eq!(events[0].task_id, "task1");
    assert_eq!(events[0].action, "submit");
}

#[test]
fn audit_sink_drops_oldest_on_overflow() {
    let mut sink = InMemoryAuditSink::new(2);

    sink.record(build_audit_event(
        "evt1", "task1", "pool1", "tenant1", "submit", None,
    ));
    sink.record(build_audit_event(
        "evt2", "task2", "pool1", "tenant1", "submit", None,
    ));
    sink.record(build_audit_event(
        "evt3", "task3", "pool1", "tenant1", "submit", None,
    ));

    let events = sink.events();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_id, "evt2"); // the first one was dropped
    assert_eq!(events[1].event_id, "evt3");
}

#[test]
fn build_audit_event_fills_every_field() {
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
