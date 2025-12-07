# Quickstart: Prometheus Parking-Lot Scheduler & Distributed Parking Management Platform

## 1) Configure pools
- Create `scheduler-config.json` with pools, limits, queue/mailbox backends (in-memory for dev, file for desktop, Postgres for cloud), and default timeouts.
- Use a standard Postgres DSN for durable queues/mailboxes/audit (e.g., `postgres://user:pass@localhost:5432/parking`).
- Ensure resource units align with capacity (e.g., GPU VRAM units or worker slots).

## 2) Run the scheduler
- Build/start the library host (native or web/worker adapter) with the config loaded before accepting tasks.
- Validate capacity and queue depth are enforced at startup; reject if config invalid.
- Apply SQL migrations when using Postgres:
  - Queue: `pl_queue_jobs` with priority/deadline indexes (see `src/infra/queue/postgres.rs::migrations()`).
  - Mailbox: `pl_mailbox_messages` with tenant/task indexes (see `src/infra/mailbox/postgres.rs::migrations()`).
  - Audit: `pl_audit_events` with tenant/task/pool indexes (see `src/core/audit.rs::PostgresAuditSink::migrations()`).

## 3) Submit tasks
- Call `POST /pools/{poolId}/tasks` with `taskId`, `priority`, `resourceCost`, optional `deadline`, and optional `mailboxKey`.
- Expect `202` with status `running` or `queued`; `409` if queue is full or capacity is zero.

## 4) Fetch status and results
- Poll `GET /pools/{poolId}/tasks/{taskId}` for live status.
- For disconnected clients, poll `GET /mailbox/{tenant}/{mailboxKey}` with optional `since`/`limit` to retrieve outcomes.

## 5) Observe and audit
- Ensure lifecycle events (submit/enqueue/start/complete/expire/reject) are emitted with task/pool/tenant subjects and retrievable within 2 seconds over 24h windows.
- Persist audits to Postgres when durability is required; otherwise use in-memory for dev.

## 6) Validate success criteria
- Load test ≥10,000 submissions to confirm zero over-capacity starts and ≤1s wake for 95% non-expired queued tasks.
- Verify 99% mailbox retrieval after disconnect/restart and audit retrieval SLA.
