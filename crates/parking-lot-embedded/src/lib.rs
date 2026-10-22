//! # Prometheus Parking Lot - embedded backends
//!
//! Crash-safe, file-backed [`LeaseQueue`](prometheus_parking_lot_core::LeaseQueue) and
//! [`ResultMailbox`](prometheus_parking_lot_core::ResultMailbox) for desktop and
//! single-node deployments (Tauri, local agents), built on the
//! [redb](https://www.redb.org/) embedded database (ADR 0004).
//!
//! ```no_run
//! # async fn demo() -> Result<(), prometheus_parking_lot_core::SchedulerError> {
//! use prometheus_parking_lot_embedded::EmbeddedStore;
//!
//! // One file holds both the queue and the mailbox.
//! let store = EmbeddedStore::open("app-data/parking-lot.redb")?;
//! let queue = store.lease_queue::<String>(1_000);
//! let mailbox = store.result_mailbox::<String>();
//! # let _ = (queue, mailbox);
//! # Ok(())
//! # }
//! ```
//!
//! Every state change (enqueue, lease, ack, nack, extend, deliver, consume,
//! evict) is exactly one redb write transaction, committed durably (fsync)
//! before the call returns. A process that dies at any point therefore leaves
//! the database in the state before or after that transaction, never between:
//! an accepted task is never lost, a leased task whose lease expires is
//! redelivered, and an acked task never comes back. Blocking storage work runs
//! on tokio's blocking pool, so these backends need a tokio runtime.
//!
//! Lease expiry and TTLs use the wall clock (`Clock::now_ms`) because they must
//! mean the same thing after a restart; a clock that steps backwards extends
//! leases by the size of the step.

mod fault;
pub mod mailbox;
pub mod queue;
mod store;

pub use mailbox::RedbResultMailbox;
pub use queue::RedbLeaseQueue;
pub use store::EmbeddedStore;
