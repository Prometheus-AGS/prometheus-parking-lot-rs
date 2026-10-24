-- Frozen 0.3 statement from mailbox/sql.rs; bind the seven legacy parameters.
WITH claim AS (
    INSERT INTO pl_result_mailbox_keys (mailbox_name, idempotency_key, forget_at)
    VALUES ($1::text, $7::text, now() + $6::float8 * interval '1 second')
    ON CONFLICT (mailbox_name, idempotency_key) DO UPDATE
        SET forget_at = EXCLUDED.forget_at
        WHERE pl_result_mailbox_keys.forget_at IS NOT NULL
          AND pl_result_mailbox_keys.forget_at <= now()
    RETURNING 1
)
INSERT INTO pl_result_mailbox
    (mailbox_name, mailbox_key, task_id, status, payload, idempotency_key, expires_at)
SELECT $1::text, $2::text, $3::bigint, $4::jsonb, $5::jsonb, $7::text,
       now() + $6::float8 * interval '1 second'
FROM claim RETURNING id
