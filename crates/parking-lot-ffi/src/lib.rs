//! # Prometheus Parking Lot - mobile and desktop bindings
//!
//! A small, safe [UniFFI](https://mozilla.github.io/uniffi-rs/) surface over the
//! embedded (redb) lease queue and result mailbox, for Kotlin (Android) and Swift
//! (iOS), and for Rust hosts such as the Tauri example. The API is
//! [`open_store`] and the async methods of [`ParkingLotStore`]: enqueue, lease, ack,
//! nack, extend, depth, and deliver / consume / peek results.
//!
//! The store gives the guarantees of `prometheus-parking-lot-embedded` (ADR 0004):
//! every state change is one fsynced transaction, an accepted task survives a crash
//! or an app kill, a leased task whose lease expires is handed out again, and a
//! result is kept until the client collects it, so it reaches a client that was not
//! running when the task finished.
//!
//! ```no_run
//! # async fn demo() -> Result<(), parking_lot_ffi::FfiError> {
//! use parking_lot_ffi::{open_store, NewTask, StoreConfig, TaskPriority};
//!
//! let store = open_store(StoreConfig {
//!     path: "app-data/parking-lot.redb".into(),
//!     max_depth: 1_000,
//! })
//! .await?;
//! let outcome = store
//!     .enqueue(NewTask {
//!         task_id: 1,
//!         payload: b"summarise this".to_vec(),
//!         priority: TaskPriority::Normal,
//!         cost: Vec::new(),
//!         idempotency_key: Some("req-1".into()),
//!         mailbox: None,
//!         deadline_ms: None,
//!     })
//!     .await?;
//! assert!(!outcome.duplicate);
//! # Ok(())
//! # }
//! ```
//!
//! # Payloads
//!
//! Bytes (`ByteArray` / `Data`), never interpreted; stored as hex text (see
//! `bindings/README.md`).
//!
//! # Generating the bindings
//!
//! `bindings/generate.sh` builds this crate and runs `uniffi-bindgen` in library
//! mode; see `bindings/README.md`. Nothing generated is checked in.
//!
//! # Unsafe code
//!
//! This crate contains no hand-written `unsafe`, and `#![deny(unsafe_code)]` keeps it
//! that way: adding an `unsafe` block or `unsafe` attribute here fails the build.
//! `UniFFI`'s macros expand to `extern "C"` entry points that are `unsafe` by
//! construction (a foreign caller passes raw pointers and buffers); rustc's
//! `unsafe_code` lint does not report code produced by another crate's macros, so the
//! generated surface needs no `allow` and none is used. The soundness of that
//! generated code is `UniFFI`'s (MPL-2.0, pinned in the workspace and checked by
//! `cargo deny`), not this crate's.

#![deny(unsafe_code)]

mod error;
mod payload;
mod runtime;
mod store;
mod types;

pub use error::FfiError;
pub use store::{ParkingLotStore, open_store};
pub use types::{
    DeliveryResult, DepthInfo, EnqueueOutcome, LeaseHandle, LeasedItem, MailboxAddress,
    NackDisposition, NewTask, ResourceAmount, ResourceClass, ResultDelivery, ResultEntry,
    StoreConfig, TaskOutcome, TaskPriority,
};

// The generated scaffolding (the `extern "C"` surface foreign code links against).
// It must sit at the crate root.
uniffi::setup_scaffolding!();
