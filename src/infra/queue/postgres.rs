//! Postgres-backed queue adapter (schema and interface stubs).

use crate::core::{ScheduledTask, SchedulerError, TaskQueue};

/// Postgres queue adapter placeholder.
pub struct PostgresQueue<P> {
    max_depth: usize,
    _marker: std::marker::PhantomData<P>,
}

impl<P> PostgresQueue<P> {
    /// Create a new adapter with a max depth.
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            _marker: std::marker::PhantomData,
        }
    }

    /// Migration statements for pgmq-style queue.
    pub fn migrations() -> &'static [&'static str] {
        &[
            r#"
CREATE TABLE IF NOT EXISTS pl_queue_jobs (
    id BIGSERIAL PRIMARY KEY,
    task_id TEXT NOT NULL,
    pool TEXT NOT NULL,
    priority SMALLINT NOT NULL,
    cost_units INT NOT NULL,
    deadline_ms NUMERIC,
    payload JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_priority ON pl_queue_jobs (priority DESC, created_at);
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_deadline ON pl_queue_jobs (deadline_ms);
"#,
        ]
    }
}

impl<P> TaskQueue<P> for PostgresQueue<P> {
    fn enqueue(&mut self, _task: ScheduledTask<P>) -> Result<(), SchedulerError> {
        Err(SchedulerError::Backend(
            "postgres queue not wired to database client".into(),
        ))
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError> {
        Err(SchedulerError::Backend(
            "postgres queue not wired to database client".into(),
        ))
    }

    fn prune_expired(&mut self, _now_ms: u128) -> Result<usize, SchedulerError> {
        Ok(0)
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    fn len(&self) -> usize {
        0
    }
}
