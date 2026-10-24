# prometheus-parking-lot-embedded

Crash-safe, file-backed backends for [`prometheus_parking_lot`](https://crates.io/crates/prometheus_parking_lot),
a resource-aware scheduler for expensive work such as LLM inference, for
desktop and single-node deployments (Tauri, local agents). Most applications
reach this crate through the facade's `embedded` feature.

It is built on the [redb](https://www.redb.org/) embedded database.

## What it holds

- `EmbeddedStore`: opens one file that holds both the queue and the mailbox.
- `RedbLeaseQueue`: the durable `LeaseQueue`.
- `RedbResultMailbox`: the durable `ResultMailbox`.

Every state change is one durably committed (fsync) redb write transaction, so
a process that dies at any point leaves the database before or after that
transaction, never between. Blocking storage work runs on tokio's blocking pool,
so these backends need a tokio runtime.

## Features

| Feature | Enables |
| --- | --- |
| `fault-injection` | Test-only: lets crash-point tests stop the process at named points inside an operation. Never enable it in a deployment. |

## Where it fits

Use it with `DurableResourcePool` from `prometheus-parking-lot-runtime-tokio`.
The Postgres alternative for multi-node use is `prometheus-parking-lot-pg`.

## License

MIT. See `LICENSE-MIT`.
