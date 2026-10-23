//! Connection and publisher settings shared by [`IggyHook`](crate::IggyHook)
//! and [`IggyNotifier`](crate::IggyNotifier).

use std::fmt;
use std::time::Duration;

use prometheus_parking_lot_core::mailbox::RetryPolicy;

/// Where and how events are published.
///
/// `Debug` hides the password in `connection_string`.
#[derive(Clone)]
#[non_exhaustive]
pub struct IggyOptions {
    /// Iggy connection string, e.g. `iggy://user:password@host:8090`.
    pub connection_string: String,
    /// Stream that holds the topics. Created when missing.
    pub stream: String,
    /// Topic for lifecycle events. Created when missing.
    pub lifecycle_topic: String,
    /// Topic for `mailbox.ready` notifications. Created when missing.
    pub mailbox_topic: String,
    /// Topic for streamed task output ([`IggyStreamSink`](crate::IggyStreamSink)).
    /// Created when missing.
    pub stream_topic: String,
    /// Partitions of a topic this crate creates.
    pub partitions: u32,
    /// Tenant that partitions the lifecycle events of an [`IggyHook`](crate::IggyHook);
    /// when `None` the pool name is the partition key.
    pub tenant: Option<String>,
    /// Events the publisher buffers; beyond this events are dropped and counted.
    pub queue_capacity: usize,
    /// Longest one publish attempt (connect, topic setup and send) may take.
    pub send_timeout: Duration,
    /// Backoff between attempts of one message, and how many it gets.
    pub retry: RetryPolicy,
    /// Recently published ids remembered to skip repeats; `0` disables.
    pub dedup_window: usize,
}

impl IggyOptions {
    /// Default options for a server at `connection_string`: stream
    /// `parking-lot`, topics `lifecycle` and `mailbox.ready`, 4 partitions, a
    /// 1024-event queue, a 2 s attempt timeout, the default retry policy and a
    /// 4096-id dedup window.
    #[must_use]
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
            stream: "parking-lot".to_owned(),
            lifecycle_topic: "lifecycle".to_owned(),
            mailbox_topic: "mailbox.ready".to_owned(),
            stream_topic: "stream.chunks".to_owned(),
            partitions: 4,
            tenant: None,
            queue_capacity: 1024,
            send_timeout: Duration::from_secs(2),
            retry: RetryPolicy::default(),
            dedup_window: 4096,
        }
    }

    /// Use stream `stream`.
    #[must_use]
    pub fn with_stream(mut self, stream: impl Into<String>) -> Self {
        self.stream = stream.into();
        self
    }

    /// Publish lifecycle events to `lifecycle` and notifications to `mailbox`
    /// (topics of the configured stream).
    #[must_use]
    pub fn with_topics(mut self, lifecycle: impl Into<String>, mailbox: impl Into<String>) -> Self {
        self.lifecycle_topic = lifecycle.into();
        self.mailbox_topic = mailbox.into();
        self
    }

    /// Publish streamed task output to `topic`.
    #[must_use]
    pub fn with_stream_topic(mut self, topic: impl Into<String>) -> Self {
        self.stream_topic = topic.into();
        self
    }

    /// Partition lifecycle events by `tenant` instead of by pool.
    #[must_use]
    pub fn with_tenant(mut self, tenant: impl Into<String>) -> Self {
        self.tenant = Some(tenant.into());
        self
    }

    /// Buffer at most `capacity` events (at least 1).
    #[must_use]
    pub const fn with_queue_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = if capacity == 0 { 1 } else { capacity };
        self
    }

    /// Bound one publish attempt to `timeout`.
    #[must_use]
    pub const fn with_send_timeout(mut self, timeout: Duration) -> Self {
        self.send_timeout = timeout;
        self
    }

    /// Replace the retry policy.
    #[must_use]
    pub const fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Remember `window` recently published ids (`0` disables).
    #[must_use]
    pub const fn with_dedup_window(mut self, window: usize) -> Self {
        self.dedup_window = window;
        self
    }

    /// Partition count of created topics (at least 1).
    #[must_use]
    pub const fn with_partitions(mut self, partitions: u32) -> Self {
        self.partitions = if partitions == 0 { 1 } else { partitions };
        self
    }
}

impl fmt::Debug for IggyOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IggyOptions")
            .field("connection_string", &redact(&self.connection_string))
            .field("stream", &self.stream)
            .field("lifecycle_topic", &self.lifecycle_topic)
            .field("mailbox_topic", &self.mailbox_topic)
            .field("stream_topic", &self.stream_topic)
            .field("partitions", &self.partitions)
            .field("tenant", &self.tenant)
            .field("queue_capacity", &self.queue_capacity)
            .field("send_timeout", &self.send_timeout)
            .field("retry", &self.retry)
            .field("dedup_window", &self.dedup_window)
            .finish()
    }
}

/// `scheme://user:password@host` with the password replaced.
fn redact(connection_string: &str) -> String {
    let Some((scheme, rest)) = connection_string.split_once("://") else {
        return connection_string.to_owned();
    };
    let Some((credentials, host)) = rest.rsplit_once('@') else {
        return connection_string.to_owned();
    };
    credentials.split_once(':').map_or_else(
        || connection_string.to_owned(),
        |(user, _)| format!("{scheme}://{user}:***@{host}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_the_password() {
        let options = IggyOptions::new("iggy://app:s3cret@iggy.internal:8090");
        let shown = format!("{options:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("app:***@iggy.internal:8090"), "{shown}");
    }
}
