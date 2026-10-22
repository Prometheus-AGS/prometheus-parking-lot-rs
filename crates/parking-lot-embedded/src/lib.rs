//! # Prometheus Parking Lot - embedded backends
//!
//! File-backed queue and mailbox for desktop and single-node deployments
//! (Tauri, local agents). They use only the standard library's file IO and
//! implement the `TaskQueue` and `Mailbox` contracts from
//! `prometheus-parking-lot-core`.
//!
//! The current backends are the `yaque`-named append-only file stores; cr-28
//! replaces them with a redb-backed lease queue.

pub mod mailbox;
pub mod queue;

pub use mailbox::YaqueMailbox;
pub use queue::YaqueQueue;
