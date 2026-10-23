//! Stream a task's tokens through Apache Iggy and resume after a disconnect.
//!
//! The executor publishes each token as a numbered chunk with an
//! `IggyStreamSink`, keyed by the task's idempotency key, and an end marker
//! when it finishes. The client reads the first tokens, disconnects, and
//! reconnects with the last sequence number it saw: it receives exactly the
//! remaining tokens, in order, then the end marker, with no gap and no repeat.
//!
//! ## Setup
//!
//! 1. Start an Iggy 0.9 server (see `examples/iggy_events.rs`).
//! 2. Run (`IGGY_URL` defaults to `iggy://iggy:iggy@127.0.0.1:8090`):
//!    ```bash
//!    cargo run --example iggy_stream_replay --features iggy
//!    ```
//!
//! Guide: `website/docs/guides/streaming-replay.md`.

use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::builders::SharedInMemoryMailbox;
use prometheus_parking_lot::core::{
    PoolLimits, ResourcePool, ScheduledTask, TaskError, TaskExecutor, TaskMetadata,
};
use prometheus_parking_lot::infra::InMemoryQueue;
use prometheus_parking_lot::infra::events::{
    IggyOptions, IggyStreamReader, IggyStreamSink, ResumePoint, StreamCursor, StreamItem,
    StreamOutcome,
};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::{MailboxKey, Priority, ResourceCost, ResourceKind, now_ms};

const TOKENS: u64 = 20;

/// Generates `TOKENS` tokens and streams each one as it is produced.
#[derive(Clone)]
struct StreamingModel {
    sink: IggyStreamSink,
}

#[async_trait]
impl TaskExecutor<String, u64> for StreamingModel {
    async fn execute(&self, prompt: String, meta: TaskMetadata) -> Result<u64, TaskError> {
        // The stream key is the task's idempotency key: the client holds it.
        let key = meta.idempotency_key.unwrap_or_else(|| meta.id.to_string());
        let mut writer = self.sink.writer(key);
        for i in 1..=TOKENS {
            tokio::time::sleep(Duration::from_millis(25)).await;
            writer
                .send(format!("{prompt}-token-{i}"))
                .await
                .map_err(TaskError::executor)?;
        }
        writer
            .finish(StreamOutcome::Completed)
            .await
            .map_err(TaskError::executor)?;
        Ok(TOKENS)
    }
}

/// Read up to `limit` items, or to the end marker; each read waits at most 30 s.
async fn read(
    cursor: &mut StreamCursor<'_>,
    limit: usize,
) -> Result<Vec<StreamItem>, Box<dyn std::error::Error + Send + Sync>> {
    let mut items = Vec::new();
    while items.len() < limit {
        match tokio::time::timeout(Duration::from_secs(30), cursor.next()).await?? {
            Some(item) => items.push(item),
            None => break,
        }
    }
    Ok(items)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let url =
        std::env::var("IGGY_URL").unwrap_or_else(|_| "iggy://iggy:iggy@127.0.0.1:8090".into());
    let options = IggyOptions::new(url).with_stream("parking-lot-demo");
    let sink = IggyStreamSink::start(&options)?;

    let mailbox = SharedInMemoryMailbox::<u64>::new();
    let pool = ResourcePool::new(
        PoolLimits {
            max_units: 1,
            max_queue_depth: 8,
            default_timeout: Duration::from_secs(60),
        },
        InMemoryQueue::new(8),
        mailbox.clone(),
        StreamingModel { sink: sink.clone() },
        TokioSpawner::new(tokio::runtime::Handle::current()),
    );

    // A per-run key, so reruns against the same server start fresh streams.
    let stream_key = format!("chat-{}", now_ms());
    let inbox = MailboxKey {
        tenant: "acme".into(),
        user_id: Some("ana".into()),
        session_id: Some("chat-1".into()),
    };
    pool.submit(ScheduledTask {
        meta: TaskMetadata {
            id: 1,
            mailbox: Some(inbox.clone()),
            priority: Priority::Normal,
            cost: ResourceCost::new(ResourceKind::GpuVram, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: Some(stream_key.clone()),
            sequence: 0,
        },
        payload: "hello".to_owned(),
    })
    .await?;

    // First connection: read five tokens, remember where we were, go away.
    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.open(&stream_key);
    let head = read(&mut cursor, 5).await?;
    let saved: ResumePoint = head
        .last()
        .map(StreamItem::resume_point)
        .unwrap_or_default();
    println!(
        "first connection got {} chunks, resume after seq {}",
        head.len(),
        saved.after_seq
    );
    drop(cursor);
    drop(reader);

    // Reconnect later with the saved point: the rest, once each, then the end.
    tokio::time::sleep(Duration::from_millis(200)).await;
    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.resume(&stream_key, saved);
    let tail = read(&mut cursor, usize::MAX).await?;
    let mut seqs = Vec::new();
    for item in &tail {
        match item {
            StreamItem::Chunk(chunk) => seqs.push(chunk.seq),
            StreamItem::End(end) => println!("end marker at seq {}: {:?}", end.seq, end.outcome),
        }
    }
    println!(
        "second connection got chunks {:?}..={:?}",
        seqs.first(),
        seqs.last()
    );
    assert_eq!(seqs, (6..=TOKENS).collect::<Vec<_>>(), "no gap, no repeat");
    assert!(matches!(tail.last(), Some(StreamItem::End(_))));

    // The task's own result is in the mailbox as usual.
    let mut outcome = None;
    for _ in 0..500 {
        outcome = mailbox.take(&inbox);
        if outcome.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    println!("mailbox: {outcome:?}");
    assert!(sink.flush(Duration::from_secs(30)).await);
    Ok(())
}
