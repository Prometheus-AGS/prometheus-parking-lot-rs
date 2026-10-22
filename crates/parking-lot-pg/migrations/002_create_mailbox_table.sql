-- Migration: Create pl_mailbox_messages table for result storage
-- This table stores task results for later retrieval by clients

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

-- Index for efficient mailbox lookups by key
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_lookup
    ON pl_mailbox_messages (tenant, user_id, session_id, created_at_ms);

-- Index for task ID lookups
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_task_id
    ON pl_mailbox_messages (task_id)
    WHERE task_id IS NOT NULL;

-- Index for pruning old messages by timestamp
CREATE INDEX IF NOT EXISTS idx_pl_mailbox_messages_created_at
    ON pl_mailbox_messages (created_at);

COMMENT ON TABLE pl_mailbox_messages IS 'Mailbox for storing task results and status updates';
COMMENT ON COLUMN pl_mailbox_messages.tenant IS 'Tenant identifier for multi-tenancy';
COMMENT ON COLUMN pl_mailbox_messages.status IS 'Task status: Queued, Running, Completed, Failed, Expired, Dropped';
COMMENT ON COLUMN pl_mailbox_messages.payload IS 'Task result payload (serialized as JSON)';
