-- This exact-generation owner's candidates are Failed and Queued. Completed
-- belongs only to the other address, so selecting owner A + Completed must fail.
INSERT INTO pl_result_mailbox
    (id, mailbox_name, mailbox_key, task_id, status, payload, idempotency_key,
     delivered_at, expires_at) OVERRIDING SYSTEM VALUE
VALUES
    (9, 'legacy', '{"tenant":"other-owner","user_id":"user","session_id":null}',
     109, '"Completed"', NULL, 'retained-failed',
     '2026-10-24 11:00:02.123456+00', '2099-10-24 11:00:00.654321+00'),
    (10, 'legacy', '{"tenant":"tenant-failed","user_id":"user","session_id":null}',
     110, '"Queued"', NULL, 'retained-failed',
     '2026-10-24 11:00:03.123456+00', '2099-10-24 11:00:00.654321+00');
