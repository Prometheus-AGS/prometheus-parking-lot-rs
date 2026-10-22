//! Shared utilities.
//!
//! `clock` and `serde` re-export the core crate's `contracts::clock` and
//! `contracts::types` modules (ADR 0001); `telemetry` stays in the facade.

pub use prometheus_parking_lot_core::contracts::clock;
pub use prometheus_parking_lot_core::contracts::types as serde;
pub mod telemetry;

pub use self::clock::*;
pub use self::serde::*;
pub use self::telemetry::*;
