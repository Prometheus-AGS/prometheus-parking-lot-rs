//! Postgres audit sink (schema definitions; DB I/O not wired).

use prometheus_parking_lot_core::hooks::audit::{AuditEvent, AuditSink};

/// Postgres-backed audit sink (schema-only; DB I/O not wired).
pub struct PostgresAuditSink;

impl PostgresAuditSink {
    /// Returns SQL migration statements for the audit log.
    #[must_use]
    pub const fn migrations() -> &'static [&'static str] {
        &[r"
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
"]
    }
}

impl AuditSink for PostgresAuditSink {
    fn record(&mut self, _event: AuditEvent) {
        // Stub: actual DB writes require a runtime + client; left to integration layer.
    }
}
