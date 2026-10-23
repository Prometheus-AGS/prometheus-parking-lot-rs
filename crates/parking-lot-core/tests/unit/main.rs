//! Unit tests for the core crate (errors, utilities, audit, in-memory mailbox).
//!
//! One test target (`--test unit`): Cargo auto-discovers `tests/unit/main.rs`
//! and the modules below are its siblings (cr-35).

mod audit_test;
mod error_test;
mod mailbox_test;
mod util_test;
