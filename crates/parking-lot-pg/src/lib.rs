//! # Prometheus Parking Lot - Postgres backends
//!
//! Postgres implementations of the async backend contracts from
//! `prometheus-parking-lot-core`, on raw sqlx with no ORM and no `block_on`,
//! plus the Postgres audit sink schema and the SQL migrations under
//! `migrations/`.
//!
//! - [`PgLeaseQueue`] is the durable
//!   [`LeaseQueue`](prometheus_parking_lot_core::LeaseQueue) (cr-26).
//! - [`PgResultMailbox`] is the durable
//!   [`ResultMailbox`](prometheus_parking_lot_core::ResultMailbox) (cr-27).
//!
//! Most applications reach them through the `prometheus_parking_lot` facade's
//! `postgres` feature.

pub mod audit;
pub mod lease_queue;
pub mod mailbox;

pub use audit::PostgresAuditSink;
pub use lease_queue::{NOTIFY_CHANNEL, PgLeaseQueue, QueueListener, Reaped, ReaperHandle, Wake};
pub use mailbox::{PgResultMailbox, SweeperHandle};
/// The connection pool the backends take.
pub use sqlx::PgPool;
