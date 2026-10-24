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
    /// Partitions of a topic this crate creates. A topic that already exists
    /// keeps its own count, and streams use that real count (read from the
    /// server), not this one.
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
    /// How long the server keeps messages of a topic this crate creates;
    /// `None` keeps them until the server's size limits remove them. Only
    /// applies when the topic is created: an existing topic keeps its expiry.
    pub retention: Option<Duration>,
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
            retention: None,
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

    /// Have the server expire messages of created topics after `retention`.
    ///
    /// The default keeps messages indefinitely (`IggyExpiry::NeverExpire`), so
    /// a long-lived deployment must size the server's disk, or set this, or
    /// remove old messages itself. Applies to topics this crate creates; an
    /// existing topic keeps the expiry it was created with.
    #[must_use]
    pub const fn with_retention(mut self, retention: Duration) -> Self {
        self.retention = Some(retention);
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
            .field("retention", &self.retention)
            .finish()
    }
}

/// `scheme://user:password@host` with the password replaced. A userinfo with
/// no colon is a personal access token (`iggy://<token>@host:port`), a secret
/// in its entirety, so all of it is replaced.
fn redact(connection_string: &str) -> String {
    let Some((scheme, rest)) = connection_string.split_once("://") else {
        return connection_string.to_owned();
    };
    let Some((credentials, host)) = rest.rsplit_once('@') else {
        return connection_string.to_owned();
    };
    credentials.split_once(':').map_or_else(
        || format!("{scheme}://***@{host}"),
        |(user, _)| format!("{scheme}://{user}:***@{host}"),
    )
}

/// Check that the Iggy client can parse `connection_string`, without
/// connecting and without building a client.
///
/// A string the client rejects makes every publish fail, which the off-path
/// publisher reports only as counted failures; configuration loading calls this
/// so the mistake is found at startup.
///
/// This runs the same grammar the client does (`iggy_common::ConnectionString`
/// for the string's transport) and stops there. Building the client would do
/// more than parse: a `iggy+quic://` client resolves its address and binds a
/// UDP socket, which needs a tokio runtime and has side effects a validator
/// must not have.
///
/// # Errors
///
/// The parser's reason, which never includes the credentials.
pub fn validate_connection_string(connection_string: &str) -> Result<(), String> {
    use iggy_common::{
        ConnectionString, ConnectionStringUtils, HttpConnectionStringOptions,
        QuicConnectionStringOptions, TcpConnectionStringOptions, TransportProtocol,
        WebSocketConnectionStringOptions,
    };

    let protocol = ConnectionStringUtils::parse_protocol(connection_string)
        .map_err(|error| error.to_string())?;
    match protocol {
        TransportProtocol::Tcp => {
            ConnectionString::<TcpConnectionStringOptions>::new(connection_string).map(drop)
        }
        TransportProtocol::Quic => {
            ConnectionString::<QuicConnectionStringOptions>::new(connection_string).map(drop)
        }
        TransportProtocol::Http => {
            ConnectionString::<HttpConnectionStringOptions>::new(connection_string).map(drop)
        }
        TransportProtocol::WebSocket => {
            ConnectionString::<WebSocketConnectionStringOptions>::new(connection_string).map(drop)
        }
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_strings_are_parsed_without_connecting() {
        assert!(validate_connection_string("iggy://app:s3cret@127.0.0.1:8090").is_ok());
        assert!(validate_connection_string("iggy://pat-token@127.0.0.1:8090").is_ok());
        for bad in ["", "not-a-url", "iggy://127.0.0.1:8090", "iggy://a:b@host"] {
            assert!(validate_connection_string(bad).is_err(), "{bad}");
        }
    }

    /// Parsing builds no transport client, so it needs no runtime and binds no
    /// socket: these are plain `#[test]`s, with no tokio runtime on the thread.
    #[test]
    fn every_transport_is_parsed_without_a_runtime() {
        for good in [
            "iggy://app:s3cret@127.0.0.1:8090",
            "iggy+tcp://app:s3cret@127.0.0.1:8090?tls=true",
            "iggy+quic://app:s3cret@iggy.invalid:8080",
            "iggy+quic://pat-token@iggy.invalid:8080?validate_certificate=false",
            "iggy+http://app:s3cret@iggy.invalid:3000",
            "iggy+ws://app:s3cret@iggy.invalid:8092",
        ] {
            assert_eq!(validate_connection_string(good), Ok(()), "{good}");
        }
        for bad in [
            "iggy+quic://app:s3cret@iggy.invalid",
            "iggy+quic://app:s3cret@iggy.invalid:8080?bogus=1",
            "iggy+http://app:s3cret@iggy.invalid:port",
            "iggy+carrier-pigeon://app:s3cret@iggy.invalid:1",
            "iggy+ws://app:@iggy.invalid:1",
        ] {
            assert!(validate_connection_string(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejection_reasons_never_echo_the_secret() {
        for bad in [
            "iggy://app:se?cr3t@iggy",
            "iggy://app:se#cr3t@iggy:notaport",
            "iggy+quic://app:se/cr3t@iggy",
            "iggy://pat-cr3t@iggy",
            "iggy+tcp://app:se:cr3t@iggy:8090",
            "iggy://app:se@cr3t@iggy:8090",
            "iggy://app:cr3t@iggy:8090?bogus=cr3t",
            "iggy://app:cr3t@iggy:8090?tls=cr3t=x",
        ] {
            let reason = validate_connection_string(bad).expect_err(bad);
            assert!(!reason.contains("cr3t"), "{bad} -> {reason}");
        }
    }

    /// Credentials are read as everything before the only `@`, so `?`, `#`
    /// and `/` are valid inside them; redaction must cover them all.
    #[test]
    fn debug_hides_credentials_holding_reserved_characters() {
        for reserved in ["?", "#", "/", "@", ":"] {
            for url in [
                format!("iggy://app:se{reserved}cr3t@iggy:8090"),
                format!("iggy://pat{reserved}cr3t@iggy:8090"),
                format!("iggy+quic://app:se{reserved}cr3t@iggy:8090?validate_certificate=false"),
            ] {
                let shown = format!("{:?}", IggyOptions::new(url.clone()));
                assert!(!shown.contains("cr3t"), "{url} -> {shown}");
            }
        }
    }

    #[test]
    fn debug_hides_the_password() {
        let options = IggyOptions::new("iggy://app:s3cret@iggy.internal:8090");
        let shown = format!("{options:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("app:***@iggy.internal:8090"), "{shown}");
    }

    #[test]
    fn debug_hides_a_personal_access_token() {
        // iggy 0.11 accepts `iggy://<token>@host:port`: one credential, no colon.
        let options = IggyOptions::new("iggy://pat-s3cret-token@iggy.internal:8090");
        let shown = format!("{options:?}");
        assert!(!shown.contains("s3cret"), "{shown}");
        assert!(shown.contains("***@iggy.internal:8090"), "{shown}");
    }
}
