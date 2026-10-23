//! Apache Iggy lifecycle events and mailbox notifications for
//! `prometheus_parking_lot` (cr-32).
//!
//! Iggy is used for **events and notification only**. It is not the mailbox
//! storage (no key lookup, no per-message delete) and not the priority work
//! queue (no priorities, no per-message ack); the results themselves stay in
//! Postgres or redb and Iggy carries a pointer event (plan section 8).
//!
//! - [`IggyHook`] is a [`Hook`](prometheus_parking_lot_core::Hook): register it
//!   with a pool's [`HookBus`](prometheus_parking_lot_core::HookBus) and every
//!   lifecycle event is published to the `lifecycle` topic, partitioned by
//!   tenant (or by pool when no tenant is set) so one tenant's events stay
//!   ordered.
//! - [`IggyNotifier`] is a [`Notifier`](prometheus_parking_lot_core::Notifier):
//!   wrap it in a [`BackgroundNotifier`](prometheus_parking_lot_core::mailbox::BackgroundNotifier)
//!   (with [`TokioHandleSpawner`] and [`TokioSleeper`]) and hand it to a
//!   [`StorageMailbox`](prometheus_parking_lot_core::StorageMailbox); each
//!   stored delivery publishes `mailbox.ready` after the result is stored, and
//!   a notifier failure never loses a stored result.
//!
//! - [`IggyStreamSink`] publishes a task's streamed output (for example LLM
//!   tokens) as ordered, numbered chunks and an end marker; an
//!   [`IggyStreamReader`] resumes a stream from the last sequence number a
//!   client saw, with no gap and no duplicate (see [`stream`]).
//!
//! # The publisher
//!
//! Neither type talks to the network on the caller's thread. Each owns a
//! bounded queue drained by one background task ([`Publisher`]):
//!
//! - [`Hook::on_event`](prometheus_parking_lot_core::Hook::on_event) only
//!   serializes the event and does a non-blocking enqueue;
//! - while the server is slow or down the queue fills, and further events are
//!   dropped and **counted** ([`PublisherStats::dropped`]), never awaited;
//! - a failed send is retried with the core [`RetryPolicy`] backoff and
//!   counted ([`PublisherStats::retried`], [`PublisherStats::failed`]).
//!
//! # Message ids
//!
//! Every message carries a deterministic 128-bit id, a `UUIDv5` of the task's
//! stable key (the idempotency key, or a key derived from the task), so a
//! retried publish carries the same id ([`ids`]). Iggy 0.9 deduplicates retried
//! requests per client rather than by message id, so the publisher also keeps a
//! bounded window of recently published ids and skips repeats
//! ([`PublisherStats::deduplicated`]); a consumer that must be exactly-once
//! across publisher restarts de-duplicates on the message id.
//!
//! The crate depends on Tokio and the Iggy client; the core crate stays
//! runtime-free.

mod hook;
pub mod ids;
mod notifier;
mod options;
mod publisher;
mod runtime;
pub mod stream;

pub use hook::IggyHook;
pub use notifier::IggyNotifier;
pub use options::IggyOptions;
pub use publisher::{Publisher, PublisherStats};
pub use runtime::{TokioHandleSpawner, TokioSleeper};
pub use stream::{
    IggyStreamReader, IggyStreamSink, ResumePoint, StreamChunk, StreamCursor, StreamEnd,
    StreamError, StreamItem, StreamOutcome, StreamWriter, partition_for,
};

/// Re-exported so applications name the retry schedule without a direct core
/// import.
pub use prometheus_parking_lot_core::mailbox::RetryPolicy;
