//! # Prometheus Parking Lot - Postgres backends
//!
//! Postgres implementations of the `TaskQueue` and `Mailbox` contracts from
//! `prometheus-parking-lot-core`, their `SeaORM` entities, the Postgres audit
//! sink schema, and the SQL migrations under `migrations/`.
//!
//! These backends bridge the synchronous contracts onto async `SeaORM` calls
//! and are replaced by the raw-sqlx lease queue (cr-26) and mailbox (cr-27).
//! Most applications reach them through the `prometheus_parking_lot` facade's
//! `postgres` feature.

pub mod audit;
pub mod entities;
pub mod mailbox;
pub mod queue;

pub use audit::PostgresAuditSink;
pub use mailbox::{MailboxMessageDto, PostgresMailbox};
pub use queue::PostgresQueue;
