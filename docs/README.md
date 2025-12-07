# Documentation Index

- [Architecture](ARCHITECTURE.md)
- [Comprehensive Architectural Report](Comprehensive Architectural Report Distributed Parking Management Systems and High-Performance Rust Audit Infrastructur.md)
- [Discussion](DISCUSSION.md)
- [API Contracts](../specs/001-parking-system-build/contracts/openapi.yaml)
- [Quickstart](../specs/001-parking-system-build/quickstart.md)
- [Load Test Harness Notes](../examples/loadtest/README.md)

## Sample Scheduler Config

See `examples/configs/scheduler-config.json` for a starting point:

```json
{
  "pools": {
    "llm_inference": {
      "max_units": 20,
      "max_queue_depth": 1000,
      "default_timeout_secs": 60,
      "queue": "postgres",
      "mailbox": "postgres",
      "runtime": "native"
    }
  }
}
```

## Migrations (Postgres)

- Queue: see `src/infra/queue/postgres.rs::migrations()` for `pl_queue_jobs`.
- Mailbox: see `src/infra/mailbox/postgres.rs::migrations()` for `pl_mailbox_messages`.
- Audit: see `src/core/audit.rs::PostgresAuditSink::migrations()` for `pl_audit_events`.
