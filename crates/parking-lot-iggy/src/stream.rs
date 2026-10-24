//! Streaming-token replay (cr-33): publish a task's streamed output to Iggy and
//! let a reconnecting client resume without a gap or a duplicate.
//!
//! # Layout
//!
//! A stream is identified by a **stream key**, which should be the task's
//! idempotency key: it is what a client already holds, and what the
//! `mailbox.ready` notification carries. Every message of one stream goes to
//! one partition of the `stream.chunks` topic, chosen by a hash of the key
//! ([`partition_for`]) over the topic's **real** partition count (read from the
//! server, so a topic created with a different count than the options say still
//! works) and set explicitly, so a reader knows where to look and per-partition
//! ordering is per-stream ordering.
//!
//! Each message has a **sequence number** `seq` that starts at 1 and rises by
//! one per message; the last message is an end marker carrying the final
//! [`StreamOutcome`]. A message's id is the `UUIDv5` of the key and `seq`.
//!
//! # Gap-free, duplicate-free resume
//!
//! Iggy offsets are per partition and the partition holds other streams too, so
//! the **sequence number is the client's cursor** and the offset is only a hint
//! for where to start reading ([`ResumePoint`]). A [`StreamCursor`] delivers
//! message `last + 1` and nothing else:
//!
//! - `seq <= last` is a duplicate (a retried publish, or a restarted executor
//!   re-publishing) and is skipped;
//! - `seq > last + 1` is a gap and is an error ([`StreamError::Gap`]), never
//!   silently passed on, so a wrong offset hint cannot silently lose a chunk
//!   once later messages arrive.
//!
//! A hint that is **too high** is not detected by itself: if no message follows
//! it (the stream already ended, say) `next` waits rather than failing. Take the
//! hint from [`StreamItem::resume_point`] or omit it.
//!
//! A reader that kept `(seq, offset)` of the last message it saw asks for
//! [`IggyStreamReader::resume`] and receives exactly the messages after it, in
//! order, then live ones until the end marker.
//!
//! # Backpressure, not drops
//!
//! Lifecycle events may be dropped when the queue is full; a stream cannot, a
//! dropped chunk is a gap. [`StreamWriter::send`] therefore **waits** for room
//! in the bounded queue, which slows the executor to the publisher's pace. It
//! runs in the executor's own task, never on the scheduler's dispatch path. If
//! a chunk is abandoned after its retries, the writer is poisoned: later chunks
//! are not published (so the stream never continues past a hole) and `send`
//! and `finish` return [`StreamError::Poisoned`].

use std::collections::VecDeque;
use std::error::Error;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use iggy::prelude::{
    Client, Consumer, Identifier, IggyClient, IggyError, MessageClient, PollingStrategy,
};
use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;

use crate::ids::message_id;
use crate::options::IggyOptions;
use crate::publisher::{Envelope, Publisher, PublisherStats, topic_partitions};

/// Messages read per poll.
const POLL_BATCH: u32 = 100;

/// The partition of `topic` that holds stream `key`.
#[must_use]
pub fn partition_for(key: &str, partitions: u32) -> u32 {
    let partitions = u128::from(partitions.max(1));
    u32::try_from(message_id("stream.partition", key) % partitions).unwrap_or(0)
}

/// How a stream ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum StreamOutcome {
    /// The task finished; its result is in the mailbox.
    Completed,
    /// The task failed.
    Failed {
        /// Why.
        message: String,
    },
    /// The task was cancelled.
    Cancelled,
}

/// Why a stream operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StreamError {
    /// An earlier chunk was abandoned after its retries; the stream has a hole.
    Poisoned,
    /// The publisher has stopped.
    Stopped,
    /// The stream already has its end marker.
    Finished,
    /// The reader found `got` where `expected` should be: a chunk is missing.
    Gap {
        /// The sequence number that should have come next.
        expected: u64,
        /// The one that did.
        got: u64,
    },
    /// The Iggy client failed.
    Backend(String),
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Poisoned => f.write_str("a chunk was abandoned; the stream has a gap"),
            Self::Stopped => f.write_str("the stream publisher has stopped"),
            Self::Finished => f.write_str("the stream already has its end marker"),
            Self::Gap { expected, got } => {
                write!(f, "stream gap: expected seq {expected}, found {got}")
            }
            Self::Backend(why) => write!(f, "iggy: {why}"),
        }
    }
}

impl Error for StreamError {}

#[derive(Serialize, Deserialize)]
struct Wire {
    key: String,
    seq: u64,
    #[serde(flatten)]
    body: WireBody,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WireBody {
    Chunk { data: String },
    End { outcome: StreamOutcome },
}

/// Publishes task output streams; shared by many [`StreamWriter`]s.
#[derive(Debug, Clone)]
pub struct IggyStreamSink {
    publisher: Arc<Publisher>,
}

impl IggyStreamSink {
    /// Start a sink on the Tokio runtime the caller is in.
    ///
    /// # Errors
    ///
    /// Outside a Tokio runtime.
    pub fn start(options: &IggyOptions) -> Result<Self, tokio::runtime::TryCurrentError> {
        Ok(Self::start_on(&Handle::try_current()?, options))
    }

    /// Start a sink on `handle`'s runtime.
    #[must_use]
    pub fn start_on(handle: &Handle, options: &IggyOptions) -> Self {
        Self {
            publisher: Arc::new(Publisher::start(handle, options, &options.stream_topic)),
        }
    }

    /// A writer for stream `key`, numbering from 1.
    #[must_use]
    pub fn writer(&self, key: impl Into<String>) -> StreamWriter {
        self.writer_from(key, 1)
    }

    /// A writer for stream `key` that continues at `next_seq`; for an executor
    /// that restarted and re-publishes from a known point (repeats are skipped
    /// by readers).
    #[must_use]
    pub fn writer_from(&self, key: impl Into<String>, next_seq: u64) -> StreamWriter {
        let key = key.into();
        StreamWriter {
            publisher: Arc::clone(&self.publisher),
            key,
            next_seq: next_seq.max(1),
            poison: Arc::new(AtomicBool::new(false)),
            finished: false,
        }
    }

    /// The publisher's counters.
    #[must_use]
    pub fn stats(&self) -> PublisherStats {
        self.publisher.stats()
    }

    /// Wait until everything queued is published or given up on; see
    /// [`Publisher::flush`].
    pub async fn flush(&self, timeout: Duration) -> bool {
        self.publisher.flush(timeout).await
    }
}

/// Publishes one stream, in order.
#[derive(Debug)]
pub struct StreamWriter {
    publisher: Arc<Publisher>,
    key: String,
    next_seq: u64,
    poison: Arc<AtomicBool>,
    finished: bool,
}

impl StreamWriter {
    /// The sequence number the next message gets.
    #[must_use]
    pub const fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// Whether a chunk was abandoned after its retries.
    #[must_use]
    pub fn is_poisoned(&self) -> bool {
        self.poison.load(Ordering::Acquire)
    }

    /// Publish a chunk and return its sequence number. Waits for room in the
    /// queue (backpressure); see the module docs.
    ///
    /// # Errors
    ///
    /// [`StreamError::Poisoned`], [`StreamError::Finished`] or
    /// [`StreamError::Stopped`].
    pub async fn send(&mut self, data: impl Into<String>) -> Result<u64, StreamError> {
        self.publish(WireBody::Chunk { data: data.into() }).await
    }

    /// Publish the end marker with the final `outcome` and return its sequence
    /// number. The stream accepts nothing afterwards. Use
    /// [`IggyStreamSink::flush`] to wait until it is on the server.
    ///
    /// # Errors
    ///
    /// As [`send`](Self::send).
    pub async fn finish(&mut self, outcome: StreamOutcome) -> Result<u64, StreamError> {
        let seq = self.publish(WireBody::End { outcome }).await?;
        self.finished = true;
        Ok(seq)
    }

    async fn publish(&mut self, body: WireBody) -> Result<u64, StreamError> {
        if self.finished {
            return Err(StreamError::Finished);
        }
        if self.is_poisoned() {
            return Err(StreamError::Poisoned);
        }
        let seq = self.next_seq;
        let wire = Wire {
            key: self.key.clone(),
            seq,
            body,
        };
        let payload = serde_json::to_vec(&wire).unwrap_or_default();
        let mut envelope = Envelope::new(
            message_id("stream.chunk", &format!("{}\u{1f}{seq}", self.key)),
            self.key.clone(),
            payload,
        );
        envelope.by_stream_hash = true;
        envelope.poison = Some(Arc::clone(&self.poison));
        self.publisher
            .enqueue_wait(envelope)
            .await
            .map_err(|_| StreamError::Stopped)?;
        self.next_seq += 1;
        Ok(seq)
    }
}

/// Where to resume a stream: the last sequence number the client saw, and
/// optionally the offset just after it (a hint that avoids re-reading the
/// partition from the start).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResumePoint {
    /// Last sequence number received; `0` for a client that has seen nothing.
    pub after_seq: u64,
    /// Partition offset to start reading at; `None` starts at the beginning.
    pub offset: Option<u64>,
}

/// One chunk of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamChunk {
    /// Sequence number, from 1.
    pub seq: u64,
    /// Offset of the message in its partition.
    pub offset: u64,
    /// The streamed text.
    pub data: String,
}

/// The end of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEnd {
    /// Sequence number of the marker.
    pub seq: u64,
    /// Offset of the message in its partition.
    pub offset: u64,
    /// How the stream ended.
    pub outcome: StreamOutcome,
}

/// What a [`StreamCursor`] yields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamItem {
    /// A chunk.
    Chunk(StreamChunk),
    /// The end marker; the cursor yields nothing after it.
    End(StreamEnd),
}

impl StreamItem {
    /// The point to resume from after this item.
    #[must_use]
    pub const fn resume_point(&self) -> ResumePoint {
        let (seq, offset) = match self {
            Self::Chunk(chunk) => (chunk.seq, chunk.offset),
            Self::End(end) => (end.seq, end.offset),
        };
        ResumePoint {
            after_seq: seq,
            offset: Some(offset + 1),
        }
    }
}

/// Reads streams back from Iggy.
pub struct IggyStreamReader {
    client: IggyClient,
    stream: Identifier,
    topic: Identifier,
    stream_name: String,
    topic_name: String,
    /// Partition count from the options, used until the server reports the
    /// topic's real one.
    configured_partitions: u32,
    /// The topic's real partition count once the server has reported it; 0
    /// before.
    partitions: AtomicU32,
    poll_interval: Duration,
}

impl fmt::Debug for IggyStreamReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IggyStreamReader")
            .field("stream", &self.stream)
            .field("topic", &self.topic)
            .field("partitions", &self.partitions.load(Ordering::Relaxed))
            .finish_non_exhaustive()
    }
}

fn backend(error: impl fmt::Display) -> StreamError {
    StreamError::Backend(error.to_string())
}

impl IggyStreamReader {
    /// Connect with `options` (the same stream, topic and partition count the
    /// sink uses).
    ///
    /// # Errors
    ///
    /// [`StreamError::Backend`] when the connection or the names are invalid.
    pub async fn connect(options: &IggyOptions) -> Result<Self, StreamError> {
        let client =
            IggyClient::from_connection_string(&options.connection_string).map_err(backend)?;
        client.connect().await.map_err(backend)?;
        Ok(Self {
            client,
            stream: Identifier::from_str_value(&options.stream).map_err(backend)?,
            topic: Identifier::from_str_value(&options.stream_topic).map_err(backend)?,
            stream_name: options.stream.clone(),
            topic_name: options.stream_topic.clone(),
            configured_partitions: options.partitions.max(1),
            partitions: AtomicU32::new(0),
            poll_interval: Duration::from_millis(50),
        })
    }

    /// The topic's partition count: what the server reports once the topic
    /// exists (a topic created by someone else may differ from the options),
    /// the configured count until then.
    async fn partition_count(&self) -> u32 {
        let known = self.partitions.load(Ordering::Relaxed);
        if known != 0 {
            return known;
        }
        match topic_partitions(&self.client, &self.stream_name, &self.topic_name).await {
            Ok(Some(count)) => {
                self.partitions.store(count, Ordering::Relaxed);
                count
            }
            _ => self.configured_partitions,
        }
    }

    /// Wait `interval` between polls while a live stream has nothing new.
    #[must_use]
    pub const fn with_poll_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// A cursor over stream `key` that yields the messages after `from`.
    #[must_use]
    pub fn resume(&self, key: &str, from: ResumePoint) -> StreamCursor<'_> {
        StreamCursor::new(self, key, from)
    }

    /// A cursor over stream `key` from its first message.
    #[must_use]
    pub fn open(&self, key: &str) -> StreamCursor<'_> {
        self.resume(key, ResumePoint::default())
    }
}

/// An ordered, gap-checked, de-duplicated view of one stream.
///
/// `next` waits for live messages; wrap it in [`tokio::time::timeout`] to bound
/// the wait (a poisoned writer never sends an end marker).
#[derive(Debug)]
pub struct StreamCursor<'a> {
    reader: &'a IggyStreamReader,
    key: String,
    next_offset: u64,
    last_seq: u64,
    ready: VecDeque<StreamItem>,
    failure: Option<StreamError>,
    ended: bool,
    duplicates: u64,
}

impl<'a> StreamCursor<'a> {
    fn new(reader: &'a IggyStreamReader, key: &str, from: ResumePoint) -> Self {
        let (next_offset, last_seq) = Self::start(from);
        Self {
            reader,
            key: key.to_owned(),
            next_offset,
            last_seq,
            ready: VecDeque::new(),
            failure: None,
            ended: false,
            duplicates: 0,
        }
    }

    #[cfg(not(parking_lot_negative_control = "s-33"))]
    fn start(from: ResumePoint) -> (u64, u64) {
        (from.offset.unwrap_or(0), from.after_seq)
    }

    /// Negative control for scenario S-33: resume forgets where the client was
    /// and re-reads the partition from its start.
    #[cfg(parking_lot_negative_control = "s-33")]
    fn start(_from: ResumePoint) -> (u64, u64) {
        (0, 0)
    }

    /// Repeats skipped so far (retried or re-published chunks).
    #[must_use]
    pub const fn duplicates_skipped(&self) -> u64 {
        self.duplicates
    }

    /// The next item, waiting for it if the stream is live; `None` after the
    /// end marker.
    ///
    /// # Errors
    ///
    /// [`StreamError::Gap`] when a chunk is missing, [`StreamError::Backend`]
    /// when the client fails (resume from the last item's
    /// [`resume_point`](StreamItem::resume_point)).
    pub async fn next(&mut self) -> Result<Option<StreamItem>, StreamError> {
        loop {
            if let Some(item) = self.ready.pop_front() {
                return Ok(Some(item));
            }
            if let Some(error) = self.failure.take() {
                return Err(error);
            }
            if self.ended {
                return Ok(None);
            }
            if !self.fill().await? {
                tokio::time::sleep(self.reader.poll_interval).await;
            }
        }
    }

    /// Poll once; `true` when any message was read.
    async fn fill(&mut self) -> Result<bool, StreamError> {
        let partition = partition_for(&self.key, self.reader.partition_count().await);
        let polled = match self
            .reader
            .client
            .poll_messages(
                &self.reader.stream,
                &self.reader.topic,
                Some(partition),
                &Consumer::default(),
                &PollingStrategy::offset(self.next_offset),
                POLL_BATCH,
                false,
            )
            .await
        {
            Ok(polled) => polled,
            // The writer has not published anything yet: the stream is live
            // but empty.
            Err(
                IggyError::StreamIdNotFound(_)
                | IggyError::StreamNameNotFound(_)
                | IggyError::TopicIdNotFound(..)
                | IggyError::TopicNameNotFound(..),
            ) => return Ok(false),
            Err(error) => return Err(backend(error)),
        };
        let mut any = false;
        for message in polled.messages {
            any = true;
            self.next_offset = message.header.offset + 1;
            let Ok(wire) = serde_json::from_slice::<Wire>(&message.payload) else {
                continue;
            };
            if wire.key == self.key
                && let Err(error) = self.accept(wire, message.header.offset)
            {
                // Items before the gap are still delivered first.
                self.failure = Some(error);
                break;
            }
        }
        Ok(any)
    }

    #[cfg(not(parking_lot_negative_control = "s-33"))]
    fn accept(&mut self, wire: Wire, offset: u64) -> Result<(), StreamError> {
        if self.ended || wire.seq <= self.last_seq {
            self.duplicates += 1;
            return Ok(());
        }
        if wire.seq != self.last_seq + 1 {
            return Err(StreamError::Gap {
                expected: self.last_seq + 1,
                got: wire.seq,
            });
        }
        self.push(wire, offset);
        Ok(())
    }

    /// Negative control for scenario S-33: no sequence tracking, so repeats and
    /// holes pass straight through.
    #[cfg(parking_lot_negative_control = "s-33")]
    fn accept(&mut self, wire: Wire, offset: u64) -> Result<(), StreamError> {
        if !self.ended {
            self.push(wire, offset);
        }
        Ok(())
    }

    fn push(&mut self, wire: Wire, offset: u64) {
        self.last_seq = wire.seq;
        let seq = wire.seq;
        match wire.body {
            WireBody::Chunk { data } => {
                self.ready
                    .push_back(StreamItem::Chunk(StreamChunk { seq, offset, data }));
            }
            WireBody::End { outcome } => {
                self.ended = true;
                self.ready.push_back(StreamItem::End(StreamEnd {
                    seq,
                    offset,
                    outcome,
                }));
            }
        }
    }
}

#[cfg(all(test, not(parking_lot_negative_control = "s-33")))]
mod tests {
    use super::*;

    #[test]
    fn partition_is_stable_and_in_range() {
        for key in ["a", "job-1", "tenant/x"] {
            let p = partition_for(key, 4);
            assert!(p < 4);
            assert_eq!(p, partition_for(key, 4));
        }
        assert_eq!(partition_for("a", 0), 0);
    }

    #[test]
    fn wire_round_trips() {
        let wire = Wire {
            key: "k".into(),
            seq: 3,
            body: WireBody::End {
                outcome: StreamOutcome::Failed {
                    message: "boom".into(),
                },
            },
        };
        let back: Wire = serde_json::from_slice(&serde_json::to_vec(&wire).unwrap()).unwrap();
        assert_eq!(back.seq, 3);
        assert!(matches!(
            back.body,
            WireBody::End {
                outcome: StreamOutcome::Failed { .. }
            }
        ));
    }
}
