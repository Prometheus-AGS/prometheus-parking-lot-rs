//! Postgres-backed mailbox adapter (schema and interface stubs).

use crate::core::{Mailbox, SchedulerError, TaskStatus};
use crate::util::serde::MailboxKey;

/// Postgres mailbox adapter placeholder.
pub struct PostgresMailbox<P> {
    _marker: std::marker::PhantomData<P>,
}

impl<P> PostgresMailbox<P> {
    /// Create a new adapter.
    pub fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    /// Migration statements for mailbox storage.
    pub fn migrations() -> &'static [&'static str] {
        &[
            r#"
CREATE TABLE IF NOT EXISTS pl_mailbox_messages (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL,
    tenant TEXT NOT NULL,
    user_id TEXT,
    session_id TEXT,
    status TEXT NOT NULL,
    payload JSONB,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_tenant ON pl_mailbox_messages (tenant, created_at);
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_task ON pl_mailbox_messages (task_id);
"#,
        ]
    }
}

impl<P> Mailbox<P> for PostgresMailbox<P> {
    fn deliver(
        &mut self,
        _key: &MailboxKey,
        _status: TaskStatus,
        _payload: Option<P>,
    ) -> Result<(), SchedulerError> {
        Err(SchedulerError::Backend(
            "postgres mailbox not wired to database client".into(),
        ))
    }
}
