-- Initial setup script run by Docker on container start
-- This combines all migrations for test environment initialization

-- Create queue table
CREATE TABLE IF NOT EXISTS pl_queue_jobs (
    id BIGSERIAL PRIMARY KEY,
    queue_name TEXT NOT NULL,
    priority SMALLINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms BIGINT NOT NULL,
    deadline_at TIMESTAMPTZ,
    deadline_ms BIGINT,
    metadata JSONB NOT NULL,
    payload JSONB NOT NULL,
    locked_by TEXT,
    locked_at TIMESTAMPTZ,
    attempts INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_dequeue 
    ON pl_queue_jobs (queue_name, priority DESC, created_at ASC);

CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_deadline 
    ON pl_queue_jobs (deadline_ms) 
    WHERE deadline_ms IS NOT NULL;

-- Create mailbox table
CREATE TABLE IF NOT EXISTS pl_mailbox_messages (
    id BIGSERIAL PRIMARY KEY,
    tenant TEXT NOT NULL,
    user_id TEXT,
    session_id TEXT,
    task_id BIGINT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    created_at_ms BIGINT NOT NULL,
    status TEXT NOT NULL,
    payload JSONB
);

CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_lookup
    ON pl_mailbox_messages (tenant, user_id, session_id, created_at_ms);

CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_task_id
    ON pl_mailbox_messages (task_id)
    WHERE task_id IS NOT NULL;
