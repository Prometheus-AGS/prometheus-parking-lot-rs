//! # Prometheus Parking Lot - Postgres backends
//!
//! Postgres implementations of the `TaskQueue` and `Mailbox` contracts from
//! `prometheus-parking-lot-core`, their `SeaORM` entities, the Postgres audit
//! sink schema, and the SQL migrations under `migrations/`.
//!
//! [`PgLeaseQueue`] is the durable async [`LeaseQueue`](prometheus_parking_lot_core::LeaseQueue)
//! on raw sqlx (cr-26). The synchronous `PostgresQueue` and `PostgresMailbox`
//! bridge onto async `SeaORM` calls and are replaced by the raw-sqlx mailbox
//! (cr-27).
//! Most applications reach them through the `prometheus_parking_lot` facade's
//! `postgres` feature.

pub mod audit;
pub mod entities;
pub mod lease_queue;
pub mod mailbox;
pub mod queue;

pub use audit::PostgresAuditSink;
pub use lease_queue::{NOTIFY_CHANNEL, PgLeaseQueue, QueueListener, Reaped, ReaperHandle, Wake};
pub use mailbox::{MailboxMessageDto, PostgresMailbox};
pub use queue::PostgresQueue;
