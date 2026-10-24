-- Frozen 0.3 queue/pgmq data. Mailbox data comes from the untouched OA07 old
-- layout, not the current schema3 API. All payloads are actual ScheduledTask
-- JSON or mailbox JSON understood by the public backends.
CREATE TABLE oa14_rehearsal_identity (
    run_id TEXT PRIMARY KEY,
    fixture_id TEXT NOT NULL CHECK (fixture_id = 'oa14-populated-0.3-pg17-v1')
);
INSERT INTO oa14_rehearsal_identity VALUES (:'run_id', 'oa14-populated-0.3-pg17-v1');

INSERT INTO pl_lease_queue
    (id, queue_name, priority, idempotency_key, meta, payload, state, attempts,
     not_before, lease_expires_at, dead_reason, enqueued_at) OVERRIDING SYSTEM VALUE
SELECT id, 'oa14_sql', priority, caller,
       jsonb_build_object('id', id, 'mailbox', NULL, 'priority', priority_name,
           'cost', '{"cpu":1}'::jsonb, 'deadline_ms', NULL, 'created_at_ms', 0,
           'idempotency_key', caller, 'sequence', id),
       to_jsonb(body), state, attempts, not_before, lease_expires_at, dead_reason,
       '2026-10-24 12:00:00.123456+00'::timestamptz
FROM (VALUES
    (501::bigint, 3::smallint, 'oa14-ready', 'critical', 'ready payload', 'ready', 0,
       '2001-01-01 00:00:00+00'::timestamptz, NULL::timestamptz, NULL::text),
    (502, 2, 'oa14-redelivery', 'high', 'expired lease payload', 'leased', 2,
       '2001-01-01 00:00:00+00', '2001-01-02 00:00:00+00', NULL),
    (503, 1, 'oa14-delayed', 'normal', 'delayed payload', 'ready', 0,
       '2099-01-01 00:00:00+00', NULL, NULL),
    (504, 0, 'oa14-dead', 'low', 'dead payload', 'dead', 3,
       '2001-01-01 00:00:00+00', NULL, 'fixture dead letter')
) AS fixture(id, priority, caller, priority_name, body, state, attempts,
             not_before, lease_expires_at, dead_reason);
SELECT setval('pl_lease_queue_id_seq', 700, true);

DO $$
DECLARE
    lane integer;
    queue text;
    message_id bigint;
    caller text;
    priority_name text;
    task jsonb;
BEGIN
    FOR lane IN 0..3 LOOP
        queue := 'pl_oa14_p' || lane;
        caller := 'oa14-pgmq-' || lane;
        priority_name := (ARRAY['low', 'normal', 'high', 'critical'])[lane + 1];
        PERFORM pgmq.create(queue);
        PERFORM pl_pgmq_tune_queue(queue);
        task := jsonb_build_object('meta', jsonb_build_object(
            'id', 800 + lane, 'mailbox', NULL, 'priority', priority_name,
            'cost', '{"cpu":1}'::jsonb, 'deadline_ms', NULL, 'created_at_ms', 0,
            'idempotency_key', caller, 'sequence', lane),
            'payload', 'pgmq lane ' || lane);
        SELECT pgmq.send(queue, task) INTO message_id;
        INSERT INTO pl_pgmq_aux (queue_name, entry_id, idem_key)
        VALUES ('oa14', message_id * 4 + lane, caller);
        IF lane = 1 THEN
            -- Actual 0.3 delayed-nack shape: message VT and auxiliary backoff.
            EXECUTE format('UPDATE pgmq.%I SET vt = $1 WHERE msg_id = $2', 'q_' || queue)
            USING '2099-01-01 00:00:00+00'::timestamptz, message_id;
            UPDATE pl_pgmq_aux SET delayed_until = '2099-01-01 00:00:00+00'
            WHERE queue_name = 'oa14' AND entry_id = message_id * 4 + lane;
        ELSIF lane = 2 THEN
            PERFORM pgmq.read(queue, 1, 1);
            EXECUTE format('UPDATE pgmq.%I SET vt = $1 WHERE msg_id = $2', 'q_' || queue)
            USING '2099-01-02 00:00:00+00'::timestamptz, message_id;
        ELSIF lane = 0 THEN
            SELECT pgmq.send(queue, task) INTO message_id;
            PERFORM pgmq.archive(queue, message_id);
        END IF;
    END LOOP;
    PERFORM pgmq.create('pl_oa14_dead');
    PERFORM pl_pgmq_tune_queue('pl_oa14_dead');
    PERFORM pgmq.send('pl_oa14_dead', '{"reason":"fixture dead letter"}'::jsonb);
END;
$$;

-- Compare full logical data and counters before any new API mutates them.
-- Keep the baseline in this same upgraded database, and archive its JSON too.
CREATE FUNCTION oa14_fixture_fingerprint() RETURNS jsonb LANGUAGE plpgsql AS $$
DECLARE
    queues jsonb := '{}'::jsonb;
    source_table record;
    rows jsonb;
BEGIN
    FOR source_table IN SELECT tablename FROM pg_tables WHERE schemaname = 'pgmq'
        ORDER BY tablename LOOP
        EXECUTE format('SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text), ''[]''::jsonb) FROM pgmq.%I r', source_table.tablename)
        INTO rows;
        queues := queues || jsonb_build_object(source_table.tablename, rows);
    END LOOP;
    RETURN jsonb_build_object(
        'queue', (SELECT jsonb_agg(to_jsonb(q) ORDER BY id) FROM pl_lease_queue q),
        'mailbox', (SELECT jsonb_agg(to_jsonb(m) ORDER BY id) FROM pl_result_mailbox m),
        'fences', (SELECT jsonb_agg(to_jsonb(k) ORDER BY mailbox_name, idempotency_key) FROM pl_result_mailbox_keys k),
        'pgmq', queues,
        'aux', (SELECT jsonb_agg(to_jsonb(a) ORDER BY queue_name, entry_id) FROM pl_pgmq_aux a),
        'sequences', (SELECT jsonb_agg(jsonb_build_object('schema', schemaname, 'name', sequencename,
            'last', last_value, 'increment', increment_by, 'cache', cache_size)
            ORDER BY schemaname, sequencename) FROM pg_sequences
            WHERE schemaname = 'pgmq' OR (schemaname = 'public' AND sequencename IN
                ('pl_lease_queue_id_seq', 'pl_result_mailbox_id_seq'))));
END;
$$;
CREATE TABLE oa14_fixture_baseline (id boolean PRIMARY KEY CHECK (id), snapshot jsonb NOT NULL);
INSERT INTO oa14_fixture_baseline VALUES (true, oa14_fixture_fingerprint());
CREATE FUNCTION oa14_assert_source_preserved() RETURNS void LANGUAGE plpgsql AS $$
BEGIN
    IF oa14_fixture_fingerprint() IS DISTINCT FROM
       (SELECT snapshot FROM oa14_fixture_baseline WHERE id) THEN
        RAISE EXCEPTION 'OA-14:source-preservation queue/mailbox/fence/pgmq/counter mismatch';
    END IF;
    IF NOT EXISTS (SELECT 1 FROM pl_result_mailbox WHERE id = 1 AND payload = '"retained"'::jsonb) THEN
        RAISE EXCEPTION 'OA-14:seeded-result missing original mailbox result';
    END IF;
END;
$$;
