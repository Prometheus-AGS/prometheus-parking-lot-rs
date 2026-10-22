//! Configuration types that the scheduling crates themselves consume.
//!
//! Scheduler-level configuration (`SchedulerConfig`, `PoolConfig`, backend
//! selection) wires backends together and therefore lives in the
//! `prometheus_parking_lot` facade.

pub mod worker_pool;

pub use worker_pool::WorkerPoolConfig;
