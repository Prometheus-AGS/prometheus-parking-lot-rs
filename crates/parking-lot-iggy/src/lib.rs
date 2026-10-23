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
//! Every message carries a 128-bit `UUIDv5` id, and a retried publish resends
//! the same message with the same id ([`ids`]). Task ids are caller-supplied and
//! can repeat, so ids are built so distinct events never share one: a
//! `mailbox.ready` id derives from the idempotency key (a result without one
//! gets a distinct id per notification), a lifecycle event gets its own id, and
//! a stream chunk's id derives from its key and sequence number.
//!
//! Message de-duplication is not the server's: the Iggy 0.9 *server* (this crate
//! uses the 0.11 *client*) deduplicates retried requests per client rather than
//! by message id, so a send that reached the server but timed out is stored
//! twice. The publisher keeps a bounded window of recently published ids and
//! skips repeats for `mailbox.ready` and stream chunks
//! ([`PublisherStats::deduplicated`]); lifecycle events bypass it. A consumer
//! that must be exactly-once, or compares counts, de-duplicates on the message
//! id.
//!
//! # Topics
//!
//! Topics are created when missing, with no expiry unless
//! [`IggyOptions::retention`] is set, so size the server's disk or set it. An
//! existing topic keeps its own expiry and partition count.
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
pub use options::{IggyOptions, validate_connection_string};
pub use publisher::{Publisher, PublisherStats};
pub use runtime::{TokioHandleSpawner, TokioSleeper};
pub use stream::{
    IggyStreamReader, IggyStreamSink, ResumePoint, StreamChunk, StreamCursor, StreamEnd,
    StreamError, StreamItem, StreamOutcome, StreamWriter, partition_for,
};

/// Re-exported so applications name the retry schedule without a direct core
/// import.
pub use prometheus_parking_lot_core::mailbox::RetryPolicy;
