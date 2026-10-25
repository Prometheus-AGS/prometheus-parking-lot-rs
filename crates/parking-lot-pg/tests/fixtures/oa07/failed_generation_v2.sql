-- Actual intermediate v2 writer shape: no status column exists in its fences.
INSERT INTO pl_result_mailbox_v2
    (id, mailbox_name, mailbox_key, task_id, status, payload, idempotency_key,
     delivered_at, expires_at) OVERRIDING SYSTEM VALUE
VALUES
    (7, 'legacy', '{"tenant":"tenant-failed","user_id":"user","session_id":null}',
     107, '{"Failed":{"kind":"executor","message":"original failure","retryable":false}}',
     NULL, 'retained-failed', '2026-10-24 11:00:00.123456+00', '2099-10-24 11:00:00.654321+00'),
    (8, 'legacy', '{"tenant":"tenant-failed","user_id":"user","session_id":null}',
     108, '{"Failed":{"kind":"executor","message":"original failure","retryable":false}}',
     NULL, 'consumed-failed', '2026-10-24 11:00:01.123456+00', NULL),
    (9, 'legacy', '{"tenant":"second-address","user_id":"user","session_id":null}',
     109, '"Running"', NULL, 'consumed-failed',
     '2026-10-24 11:00:02.123456+00', NULL);
INSERT INTO pl_result_mailbox_keys_v2 (mailbox_name, mailbox_key, idempotency_key, forget_at)
SELECT mailbox_name, mailbox_key, idempotency_key, expires_at
FROM pl_result_mailbox_v2 WHERE id IN (7, 8, 9);
DELETE FROM pl_result_mailbox_v2 WHERE id IN (8, 9);
