-- First-boot extension setup. Runs once, against POSTGRES_DB, via the official
-- entrypoint's /docker-entrypoint-initdb.d hook.
CREATE EXTENSION IF NOT EXISTS pgmq;
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;

-- Also install into template1 so databases created later (one per test, for
-- isolation) inherit pgmq without another CREATE EXTENSION.
\connect template1
CREATE EXTENSION IF NOT EXISTS pgmq;
CREATE EXTENSION IF NOT EXISTS pg_stat_statements;
