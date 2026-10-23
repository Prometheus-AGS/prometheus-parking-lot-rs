//! A pinned Apache Iggy server in a container, plus helpers to read it back.
//!
//! The image is pinned by tag and digest. The server needs the capabilities the
//! Iggy docs list for Docker (`SYS_NICE`, unconfined seccomp, unlimited
//! `memlock`); one unpinned shard because the default NUMA-aware allocation fails to
//! bind memory inside Docker Desktop's VM, and pinned shards of parallel
//! servers would fight for the same cores.

#![allow(
    dead_code,
    reason = "shared by several test binaries; each uses a subset"
)]

use std::error::Error;
use std::process::Command;
use std::time::{Duration, Instant};

use iggy::prelude::{
    Client, Consumer, Identifier, IggyClient, IggyMessage, MessageClient, PollingStrategy,
};
use testcontainers::{
    ContainerAsync, GenericImage, ImageExt,
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
};

pub type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Server image repository.
pub const IGGY_IMAGE: &str = "apache/iggy";
/// Server tag and the digest of its multi-arch manifest list (Docker Hub,
/// 2026-10-23): the Iggy 0.9.0 server, which pairs with Rust SDK 0.11.0.
pub const IGGY_TAG: &str =
    "0.9.0@sha256:b35fc2841964f2962a5f164d33b3ae3a9de99a66920defd3cd8a1c039470602d";
/// Root credentials configured in the container (test-only).
pub const IGGY_USER: &str = "iggy";
pub const IGGY_PASSWORD: &str = "iggy";

/// A running Iggy server.
pub struct IggyServer {
    pub connection_string: String,
    id: String,
    _container: ContainerAsync<GenericImage>,
}

impl IggyServer {
    /// Start a fresh server and wait until it accepts a login.
    pub async fn start() -> TestResult<Self> {
        // `free_port` can lose a race against a parallel test's container; the
        // daemon then refuses the bind, and a new port is tried.
        let mut last = None;
        for _ in 0..5 {
            match Self::start_on(free_port()?).await {
                Ok(server) => return Ok(server),
                Err(error) if error.to_string().contains("address already in use") => {
                    last = Some(error);
                }
                Err(error) => return Err(error),
            }
        }
        Err(last.unwrap_or_else(|| "no free port".into()))
    }

    async fn start_on(port: u16) -> TestResult<Self> {
        // The server publishes the address clients must dial in its cluster
        // metadata, and SDK clients reconnect to it, so the container port must
        // equal the host port it is reached on.
        // An IPv4 literal: `localhost` may resolve to ::1 first, where Docker's
        // port proxy accepts and then hangs.
        let container = GenericImage::new(IGGY_IMAGE, IGGY_TAG)
            .with_exposed_port(ContainerPort::Tcp(port))
            .with_wait_for(WaitFor::Nothing)
            .with_env_var("IGGY_ROOT_USERNAME", IGGY_USER)
            .with_env_var("IGGY_ROOT_PASSWORD", IGGY_PASSWORD)
            .with_env_var("IGGY_TCP_ADDRESS", format!("0.0.0.0:{port}"))
            .with_env_var("IGGY_NODE_ADVERTISED_ADDRESS", "localhost")
            .with_env_var("IGGY_HTTP_ENABLED", "false")
            .with_env_var("IGGY_QUIC_ENABLED", "false")
            .with_env_var("IGGY_WEBSOCKET_ENABLED", "false")
            .with_env_var("IGGY_SHARDING_CPU_ALLOCATION", "1")
            .with_env_var("IGGY_SHARDING_PIN_CORES", "false")
            .with_mapped_port(port, ContainerPort::Tcp(port))
            .with_cap_add("SYS_NICE")
            .with_security_opt("seccomp=unconfined")
            .with_ulimit("memlock", -1, Some(-1))
            .start()
            .await?;
        let connection_string = format!("iggy://{IGGY_USER}:{IGGY_PASSWORD}@127.0.0.1:{port}");
        let server = Self {
            connection_string,
            id: container.id().to_owned(),
            _container: container,
        };
        server.wait_ready().await?;
        Ok(server)
    }

    async fn wait_ready(&self) -> TestResult<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            let attempt = tokio::time::timeout(Duration::from_secs(5), self.client()).await;
            let why = match attempt {
                Ok(Ok(client)) => {
                    drop(client);
                    return Ok(());
                }
                Ok(Err(error)) => error.to_string(),
                Err(_) => "connect timed out".to_owned(),
            };
            if Instant::now() > deadline {
                return Err(
                    format!("Iggy server did not accept a login within 60 s: {why}").into(),
                );
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    /// A logged-in client.
    pub async fn client(&self) -> TestResult<IggyClient> {
        let client = IggyClient::from_connection_string(&self.connection_string)?;
        client.connect().await?;
        Ok(client)
    }

    /// Freeze the server process (`docker pause`): connections hang, nothing errors.
    pub fn pause(&self) -> TestResult<()> {
        docker(&["pause", &self.id])
    }

    /// Resume a paused server.
    pub fn unpause(&self) -> TestResult<()> {
        docker(&["unpause", &self.id])
    }
}

fn free_port() -> TestResult<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

fn docker(args: &[&str]) -> TestResult<()> {
    let status = Command::new("docker").args(args).status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("docker {args:?} failed: {status}").into())
    }
}

/// One message read back from a topic.
#[derive(Debug, Clone)]
pub struct Read {
    pub partition: u32,
    pub offset: u64,
    pub id: u128,
    pub body: serde_json::Value,
}

/// Every message of `topic` from `from_offset` on, partition by partition.
pub async fn read_topic(
    client: &IggyClient,
    stream: &str,
    topic: &str,
    partitions: u32,
    from_offset: u64,
) -> TestResult<Vec<Read>> {
    let stream_id = Identifier::from_str_value(stream)?;
    let topic_id = Identifier::from_str_value(topic)?;
    let consumer = Consumer::default();
    let mut out = Vec::new();
    for partition in 0..partitions {
        let polled = client
            .poll_messages(
                &stream_id,
                &topic_id,
                Some(partition),
                &consumer,
                &PollingStrategy::offset(from_offset),
                1000,
                false,
            )
            .await;
        let Ok(polled) = polled else { continue };
        for message in polled.messages {
            out.push(read(partition, &message));
        }
    }
    Ok(out)
}

fn read(partition: u32, message: &IggyMessage) -> Read {
    Read {
        partition,
        offset: message.header.offset,
        id: message.header.id,
        body: serde_json::from_slice(&message.payload).unwrap_or(serde_json::Value::Null),
    }
}

/// Poll `topic` until it holds at least `expected` messages (or 30 s pass).
pub async fn wait_for_messages(
    client: &IggyClient,
    stream: &str,
    topic: &str,
    partitions: u32,
    expected: usize,
) -> TestResult<Vec<Read>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let messages = read_topic(client, stream, topic, partitions, 0).await?;
        if messages.len() >= expected || Instant::now() > deadline {
            return Ok(messages);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The first copy of each message id, in read order per partition.
///
/// Delivery is at-least-once: a send that reached the server but timed out on
/// the client is retried and stored twice under the same id. Consumers (and
/// tests asserting order or counts) de-duplicate by id.
pub fn distinct_by_id(messages: Vec<Read>) -> Vec<Read> {
    let mut seen = std::collections::BTreeSet::new();
    messages
        .into_iter()
        .filter(|m| seen.insert((m.partition, m.id)))
        .collect()
}

/// Poll `topic` until it holds `expected` distinct message ids (or 30 s pass),
/// returning the de-duplicated messages.
pub async fn wait_for_distinct_messages(
    client: &IggyClient,
    stream: &str,
    topic: &str,
    partitions: u32,
    expected: usize,
) -> TestResult<Vec<Read>> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let messages = distinct_by_id(read_topic(client, stream, topic, partitions, 0).await?);
        if messages.len() >= expected || Instant::now() > deadline {
            return Ok(messages);
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
