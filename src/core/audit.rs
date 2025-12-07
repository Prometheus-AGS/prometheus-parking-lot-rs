//! Audit sink implementations.
//!
//! Provides in-memory logging and Postgres schema definitions for audit persistence.

use std::collections::VecDeque;

use crate::util::clock::now_ms;

/// Audit event structure.
#[derive(Debug, Clone)]
pub struct AuditEvent {
    /// Event identifier.
    pub event_id: String,
    /// Related task identifier.
    pub task_id: String,
    /// Pool identifier.
    pub pool: String,
    /// Tenant identifier.
    pub tenant: String,
    /// Action taken (submit, enqueue, start, complete, expire, reject).
    pub action: String,
    /// Timestamp milliseconds.
    pub created_at_ms: u128,
    /// Additional context.
    pub payload: Option<String>,
}

/// Audit sink abstraction.
pub trait AuditSink: Send {
    /// Record an audit event.
    fn record(&mut self, event: AuditEvent);
}

/// In-memory audit sink for testing and dev.
pub struct InMemoryAuditSink {
    events: VecDeque<AuditEvent>,
    max_events: usize,
}

impl InMemoryAuditSink {
    /// Create a new in-memory sink with a bounded buffer.
    pub fn new(max_events: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(max_events),
            max_events,
        }
    }

    /// Retrieve a snapshot of stored events.
    pub fn events(&self) -> Vec<AuditEvent> {
        self.events.iter().cloned().collect()
    }
}

impl AuditSink for InMemoryAuditSink {
    fn record(&mut self, event: AuditEvent) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(event);
    }
}

/// Postgres-backed audit sink (schema-only; DB I/O not wired).
pub struct PostgresAuditSink;

impl PostgresAuditSink {
    /// Returns SQL migration statements for the audit log.
    pub fn migrations() -> &'static [&'static str] {
        &[
            r#"
CREATE TABLE IF NOT EXISTS pl_audit_events (
    event_id TEXT PRIMARY KEY,
    task_id TEXT NOT NULL,
    pool TEXT NOT NULL,
    tenant TEXT NOT NULL,
    action TEXT NOT NULL,
    payload JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_pl_audit_events_tenant_created ON pl_audit_events (tenant, created_at);
CREATE INDEX IF NOT EXISTS idx_pl_audit_events_task ON pl_audit_events (task_id);
CREATE INDEX IF NOT EXISTS idx_pl_audit_events_pool ON pl_audit_events (pool);
"#,
        ]
    }
}

impl AuditSink for PostgresAuditSink {
    fn record(&mut self, _event: AuditEvent) {
        // Stub: actual DB writes require a runtime + client; left to integration layer.
    }
}

/// Helper to build an audit event from context.
pub fn build_audit_event(
    event_id: impl Into<String>,
    task_id: impl Into<String>,
    pool: impl Into<String>,
    tenant: impl Into<String>,
    action: impl Into<String>,
    payload: Option<String>,
) -> AuditEvent {
    AuditEvent {
        event_id: event_id.into(),
        task_id: task_id.into(),
        pool: pool.into(),
        tenant: tenant.into(),
        action: action.into(),
        created_at_ms: now_ms(),
        payload,
    }
}
