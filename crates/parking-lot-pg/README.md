# prometheus-parking-lot-pg

Postgres backends for [`prometheus_parking_lot`](https://crates.io/crates/prometheus_parking_lot),
a resource-aware scheduler for expensive work such as LLM inference. Most
applications reach this crate through the facade's `postgres` feature.

It uses raw sqlx (no ORM) on the tokio runtime and implements the async
contracts from `prometheus-parking-lot-core`.

## What it holds

- `PgLeaseQueue`: the durable `LeaseQueue`. `QueueListener` wakes consumers on
  Postgres `NOTIFY`; `ReaperHandle` reclaims expired leases.
- `PgmqLeaseQueue`: the same contract over the pgmq extension, through plain SQL.
- `PgResultMailbox`: the durable `ResultMailbox`. `SweeperHandle` evicts
  expired results.
- `PostgresAuditSink`: an `AuditSink` that writes events to `pl_audit_events`.
  `spawn(pool, capacity)` starts a background writer that inserts in batches;
  `record` never blocks, and a full buffer or a failed batch is counted in
  `lost()` and logged. `migrate(&pool)` creates the table; `shutdown()` flushes.
- `PgPool`: re-exported from sqlx.
- The SQL migrations under `migrations/`, shipped in the package.

## Features

This crate has no feature flags. sqlx is built with `postgres`,
`runtime-tokio`, `tls-rustls` and `json`.

## Where it fits

Use it with `ResourcePool` or `DurableResourcePool` from
`prometheus-parking-lot-runtime-tokio`. The embedded alternative for desktop and
single-node use is `prometheus-parking-lot-embedded`.

## License

MIT. See `LICENSE-MIT`.
