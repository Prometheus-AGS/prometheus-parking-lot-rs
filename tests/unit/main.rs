//! Unit tests for the facade crate (configuration and builders).
//!
//! One test target (`--test unit`): Cargo auto-discovers `tests/unit/main.rs`
//! and the modules below are its siblings (cr-35). The suites for code that
//! moved into the workspace crates live next to that code under
//! `crates/*/tests/unit/` (ADR 0001).

#[cfg(feature = "tokio-runtime")]
mod builders_test;
mod config_test;
