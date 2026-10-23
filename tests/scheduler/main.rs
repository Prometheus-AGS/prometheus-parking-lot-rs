//! Scheduler integration tests for the facade crate (`--test scheduler`).
//!
//! Durable dispatch (cr-49):
//!
//! - `durable_sigkill`: scenario S-16. A child process runs a
//!   `DurableResourcePool` on Postgres 18 (project image) or redb, accepts a
//!   task and is killed with `SIGKILL` mid-execution, or between execution and
//!   the mailbox write; a restarted pool completes the task and exactly one
//!   result is consumable. Tests are named `pool_sigkill_*`, so
//!   `-E "test(pool_sigkill_)"` selects them; the Postgres ones need Docker
//!   and are `#[ignore]`d (run with `--run-ignored all`).
//! - `durable_dispatch`: ack-after-delivery ordering, persist-before-accept and
//!   the outcome-to-lease mapping, on the in-memory reference backends.
//! - `durable_lease`: a task running for three lease durations is never
//!   leased by a second dispatcher (redb).
//!
//! Shutdown (cr-29):
//!
//! - `durable_shutdown`: scenario S-9 for the durable pool. A pool shut down
//!   under each `ShutdownPolicy` hands back (nack, requeue) what it cancels,
//!   and a restarted pool on the same redb file or Postgres database runs
//!   every task that did not complete, with one result each. Tests are named
//!   `shutdown_durable_*`; the Postgres ones need Docker and are `#[ignore]`d.
//!   The in-memory pools' S-9 tests are in
//!   `crates/parking-lot-runtime-tokio/tests/shutdown_test.rs`.
//!
//! Negative control S-16: under
//! `RUSTFLAGS='--cfg parking_lot_negative_control="s-16"'` the pool acks
//! before the mailbox write, and the `*_between_execution_and_delivery` tests
//! and `durable_ack_after_delivery_*` must fail.

#![cfg(feature = "tokio-runtime")]

mod support;

mod durable_dispatch;
#[cfg(feature = "embedded")]
mod durable_lease;
#[cfg(any(feature = "postgres", feature = "embedded"))]
mod durable_shutdown;
#[cfg(any(feature = "postgres", feature = "embedded"))]
mod durable_sigkill;
