-- Migration: Install pgmq extension for high-throughput queue operations
-- Note: This requires pgmq to be available in the PostgreSQL installation
-- For test environments, this may be skipped if using custom table queue only

-- Attempt to create the extension if available
-- This will fail gracefully if pgmq is not installed
DO $$
BEGIN
    CREATE EXTENSION IF NOT EXISTS pgmq CASCADE;
    RAISE NOTICE 'pgmq extension installed successfully';
EXCEPTION
    WHEN OTHERS THEN
        RAISE NOTICE 'pgmq extension not available, skipping: %', SQLERRM;
END
$$;

COMMENT ON EXTENSION pgmq IS 'PostgreSQL Message Queue extension for high-throughput operations';
