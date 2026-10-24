//! The bounded, off-path publisher behind [`IggyHook`](crate::IggyHook) and
//! [`IggyNotifier`](crate::IggyNotifier).
//!
//! `enqueue` is the only call a producer of events makes: a non-blocking push
//! onto a bounded queue. One background task owns the Iggy connection, takes
//! messages in order, and publishes each with a per-attempt timeout and a
//! bounded retry. A slow or unreachable server therefore shows up as a full
//! queue and counted drops, never as latency in the caller.

use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use iggy::prelude::{
    Client, DirectConfig, IggyClient, IggyExpiry, IggyMessage, IggyProducer, MaxTopicSize,
    Partitioning,
};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::options::IggyOptions;
use crate::stream::partition_for;

/// Longest partition key Iggy accepts, in bytes.
const MAX_KEY_LEN: usize = 255;

/// One message to publish.
pub struct Envelope {
    /// Deterministic message id (see [`crate::ids`]).
    pub(crate) id: u128,
    /// Partition key: messages with the same key land on the same partition.
    pub(crate) key: String,
    /// JSON payload.
    pub(crate) payload: Vec<u8>,
    /// Choose the partition by [`crate::stream::partition_for`] over the
    /// topic's real partition count instead of the server's key hash (stream
    /// replay needs a reader to know which partition holds a stream).
    pub(crate) by_stream_hash: bool,
    /// Skip this message when its id was published recently. Off for messages
    /// whose id is not a stable function of their content (lifecycle events),
    /// where a repeated id would silently drop a real event.
    pub(crate) dedup: bool,
    /// Set by the worker when the message is abandoned; a message whose flag is
    /// already set is skipped, so a stream never publishes past a gap.
    pub(crate) poison: Option<Arc<AtomicBool>>,
}

impl Envelope {
    pub(crate) const fn new(id: u128, key: String, payload: Vec<u8>) -> Self {
        Self {
            id,
            key,
            payload,
            by_stream_hash: false,
            dedup: true,
            poison: None,
        }
    }

    /// Never skip this message as a repeat.
    pub(crate) const fn without_dedup(mut self) -> Self {
        self.dedup = false;
        self
    }
}

enum Command {
    Publish {
        envelope: Envelope,
        /// Set by the S-11 negative control: the caller waits for the publish.
        done: Option<std::sync::mpsc::Sender<()>>,
    },
    Flush(oneshot::Sender<()>),
}

#[derive(Debug, Default)]
struct Counters {
    enqueued: AtomicU64,
    published: AtomicU64,
    dropped: AtomicU64,
    deduplicated: AtomicU64,
    retried: AtomicU64,
    failed: AtomicU64,
}

/// What a [`Publisher`] has done so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublisherStats {
    /// Messages accepted onto the queue.
    pub enqueued: u64,
    /// Messages the server accepted.
    pub published: u64,
    /// Messages refused because the queue was full (or the publisher stopped).
    pub dropped: u64,
    /// Messages skipped because their id was published recently.
    pub deduplicated: u64,
    /// Failed attempts that were followed by another attempt.
    pub retried: u64,
    /// Messages given up on after the retry budget.
    pub failed: u64,
}

/// A bounded queue and the background task that publishes it to one topic.
///
/// Dropping the publisher stops the task; call [`flush`](Self::flush) first to
/// let queued messages finish.
#[derive(Debug)]
pub struct Publisher {
    sender: mpsc::Sender<Command>,
    counters: Arc<Counters>,
    task: JoinHandle<()>,
}

/// Why a message was not queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnqueueError {
    /// The queue is full or the publisher has stopped.
    Rejected,
}

impl Publisher {
    /// Start publishing to `topic` of `options.stream` on `handle`'s runtime.
    /// Connects lazily, so a server that is down now does not fail the start.
    pub(crate) fn start(handle: &Handle, options: &IggyOptions, topic: &str) -> Self {
        let (sender, receiver) = mpsc::channel(options.queue_capacity.max(1));
        let counters = Arc::new(Counters::default());
        let task = handle.spawn(
            Worker {
                options: options.clone(),
                topic: topic.to_owned(),
                counters: Arc::clone(&counters),
                connection: None,
                recent: Recent::new(options.dedup_window),
            }
            .run(receiver),
        );
        Self {
            sender,
            counters,
            task,
        }
    }

    /// Queue `envelope` without blocking.
    #[cfg(not(parking_lot_negative_control = "s-11"))]
    pub(crate) fn enqueue(&self, envelope: Envelope) -> Result<(), EnqueueError> {
        let command = Command::Publish {
            envelope,
            done: None,
        };
        if self.sender.try_send(command).is_ok() {
            self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
            Ok(())
        } else {
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
            Err(EnqueueError::Rejected)
        }
    }

    /// Negative control for scenario S-11: publish synchronously, blocking the
    /// caller until the server has answered (or the retries are spent).
    #[cfg(parking_lot_negative_control = "s-11")]
    pub(crate) fn enqueue(&self, envelope: Envelope) -> Result<(), EnqueueError> {
        let (done, wait) = std::sync::mpsc::channel();
        let command = Command::Publish {
            envelope,
            done: Some(done),
        };
        if self.sender.try_send(command).is_err() {
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
            return Err(EnqueueError::Rejected);
        }
        self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
        // Park the caller until the server has answered: dispatch latency now
        // includes the network.
        let _ = wait.recv();
        Ok(())
    }

    /// Queue `envelope`, waiting for room when the queue is full.
    ///
    /// For ordered streams where a drop would leave a gap: the producer is
    /// slowed to the publisher's pace (backpressure) instead of losing data.
    /// Returns `Rejected` only when the publisher has stopped.
    pub(crate) async fn enqueue_wait(&self, envelope: Envelope) -> Result<(), EnqueueError> {
        let command = Command::Publish {
            envelope,
            done: None,
        };
        if self.sender.send(command).await.is_ok() {
            self.counters.enqueued.fetch_add(1, Ordering::Relaxed);
            Ok(())
        } else {
            self.counters.dropped.fetch_add(1, Ordering::Relaxed);
            Err(EnqueueError::Rejected)
        }
    }

    /// The counters now.
    #[must_use]
    pub fn stats(&self) -> PublisherStats {
        PublisherStats {
            enqueued: self.counters.enqueued.load(Ordering::Relaxed),
            published: self.counters.published.load(Ordering::Relaxed),
            dropped: self.counters.dropped.load(Ordering::Relaxed),
            deduplicated: self.counters.deduplicated.load(Ordering::Relaxed),
            retried: self.counters.retried.load(Ordering::Relaxed),
            failed: self.counters.failed.load(Ordering::Relaxed),
        }
    }

    /// Wait until every message queued before this call has been published or
    /// given up on, or `timeout` passes. `true` when the queue was drained.
    pub async fn flush(&self, timeout: Duration) -> bool {
        let (done, wait) = oneshot::channel();
        let drained =
            async { self.sender.send(Command::Flush(done)).await.is_ok() && wait.await.is_ok() };
        tokio::time::timeout(timeout, drained)
            .await
            .unwrap_or(false)
    }
}

impl Drop for Publisher {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A connected producer and the client that keeps its session alive.
struct Connection {
    _client: IggyClient,
    producer: IggyProducer,
    /// The topic's real partition count, read from the server.
    partitions: u32,
}

struct Worker {
    options: IggyOptions,
    topic: String,
    counters: Arc<Counters>,
    connection: Option<Connection>,
    recent: Recent,
}

impl Worker {
    async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        while let Some(command) = receiver.recv().await {
            match command {
                Command::Flush(done) => {
                    let _ = done.send(());
                }
                Command::Publish { envelope, done } => {
                    self.publish(&envelope).await;
                    if let Some(done) = done {
                        let _ = done.send(());
                    }
                }
            }
        }
    }

    async fn publish(&mut self, envelope: &Envelope) {
        if envelope
            .poison
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
        {
            self.counters.failed.fetch_add(1, Ordering::Relaxed);
            return;
        }
        if envelope.dedup && self.recent.contains(envelope.id) {
            self.counters.deduplicated.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let mut attempt = 1_u32;
        loop {
            let outcome =
                tokio::time::timeout(self.options.send_timeout, self.send_once(envelope)).await;
            let error = match outcome {
                Ok(Ok(())) => {
                    self.counters.published.fetch_add(1, Ordering::Relaxed);
                    if envelope.dedup {
                        self.recent.insert(envelope.id);
                    }
                    return;
                }
                Ok(Err(error)) => error,
                Err(_) => format!("no answer within {:?}", self.options.send_timeout),
            };
            // The session may be dead; reconnect on the next attempt.
            self.connection = None;
            if let Some(wait) = self.options.retry.backoff_after(attempt) {
                self.counters.retried.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(topic = %self.topic, attempt, ?wait, %error, "iggy publish failed; retrying");
                tokio::time::sleep(wait).await;
                attempt += 1;
            } else {
                self.counters.failed.fetch_add(1, Ordering::Relaxed);
                // OA-16 negative control: queued later chunks remain publishable
                // after a real-server rejection. The reader must detect the gap.
                #[cfg(not(parking_lot_negative_control = "oa-16"))]
                if let Some(flag) = &envelope.poison {
                    flag.store(true, Ordering::Release);
                }
                tracing::error!(topic = %self.topic, attempt, %error, "iggy publish abandoned after the retry budget");
                return;
            }
        }
    }

    async fn send_once(&mut self, envelope: &Envelope) -> Result<(), String> {
        if self.connection.is_none() {
            self.connection = Some(self.connect().await?);
        }
        let Some(connection) = self.connection.as_ref() else {
            return Err("not connected".to_owned());
        };
        let message = IggyMessage::builder()
            .id(envelope.id)
            .payload(envelope.payload.clone().into())
            .build()
            .map_err(|e| e.to_string())?;
        let partitioning = if envelope.by_stream_hash {
            Partitioning::partition_id(partition_for(&envelope.key, connection.partitions))
        } else {
            let mut key = envelope.key.as_bytes();
            key = &key[..key.len().min(MAX_KEY_LEN)];
            Partitioning::messages_key(key).map_err(|e| e.to_string())?
        };
        connection
            .producer
            .send_with_partitioning(vec![message], Some(Arc::new(partitioning)))
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn connect(&self) -> Result<Connection, String> {
        let client = IggyClient::from_connection_string(&self.options.connection_string)
            .map_err(|e| e.to_string())?;
        client.connect().await.map_err(|e| e.to_string())?;
        let producer = client
            .producer(&self.options.stream, &self.topic)
            .map_err(|e| e.to_string())?
            .direct(DirectConfig::builder().build())
            .create_stream_if_not_exists()
            .create_topic_if_not_exists(
                self.options.partitions,
                self.options
                    .retention
                    .map_or(IggyExpiry::NeverExpire, |keep| {
                        IggyExpiry::ExpireDuration(keep.into())
                    }),
                MaxTopicSize::ServerDefault,
            )
            .build();
        producer.init().await.map_err(|e| e.to_string())?;
        let partitions = topic_partitions(&client, &self.options.stream, &self.topic)
            .await
            .map_err(|e| e.to_string())?
            .unwrap_or(self.options.partitions);
        Ok(Connection {
            _client: client,
            producer,
            partitions,
        })
    }
}

/// The partition count the server reports for `topic`, `None` when the topic
/// cannot be found.
pub async fn topic_partitions(
    client: &IggyClient,
    stream: &str,
    topic: &str,
) -> Result<Option<u32>, iggy::prelude::IggyError> {
    use iggy::prelude::{Identifier, TopicClient};

    let stream = Identifier::from_str_value(stream)?;
    let topic = Identifier::from_str_value(topic)?;
    let details = client.get_topic(&stream, &topic).await?;
    Ok(details.map(|t| t.partitions_count.max(1)))
}

/// A bounded window of recently published ids.
struct Recent {
    capacity: usize,
    order: VecDeque<u128>,
    members: HashSet<u128>,
}

impl Recent {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            order: VecDeque::new(),
            members: HashSet::new(),
        }
    }

    fn contains(&self, id: u128) -> bool {
        self.members.contains(&id)
    }

    fn insert(&mut self, id: u128) {
        if self.capacity == 0 || !self.members.insert(id) {
            return;
        }
        self.order.push_back(id);
        if self.order.len() > self.capacity
            && let Some(oldest) = self.order.pop_front()
        {
            self.members.remove(&oldest);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_window_forgets_the_oldest_id() {
        let mut recent = Recent::new(2);
        recent.insert(1);
        recent.insert(2);
        recent.insert(3);
        assert!(!recent.contains(1));
        assert!(recent.contains(2) && recent.contains(3));
    }

    #[test]
    fn zero_window_remembers_nothing() {
        let mut recent = Recent::new(0);
        recent.insert(1);
        assert!(!recent.contains(1));
    }
}
