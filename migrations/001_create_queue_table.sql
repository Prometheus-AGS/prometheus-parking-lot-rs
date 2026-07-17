-- Migration: Create pl_queue_jobs table for persistent task queuing
-- This table stores queued tasks with priority ordering and deadline support

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

-- Index for efficient dequeue operations (priority DESC, FIFO within priority)
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_dequeue 
    ON pl_queue_jobs (queue_name, priority DESC, created_at ASC);

-- Index for deadline-based pruning
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_deadline 
    ON pl_queue_jobs (deadline_ms) 
    WHERE deadline_ms IS NOT NULL;

-- Index for locked tasks tracking
CREATE INDEX IF NOT EXISTS idx_pl_queue_jobs_locked 
    ON pl_queue_jobs (locked_by, locked_at) 
    WHERE locked_by IS NOT NULL;

COMMENT ON TABLE pl_queue_jobs IS 'Persistent task queue with priority and deadline support';
COMMENT ON COLUMN pl_queue_jobs.priority IS 'Task priority: 0=Low, 1=Normal, 2=High, 3=Critical';
COMMENT ON COLUMN pl_queue_jobs.metadata IS 'Task metadata (TaskMetadata serialized as JSON)';
COMMENT ON COLUMN pl_queue_jobs.payload IS 'Task payload (user-defined, serialized as JSON)';
