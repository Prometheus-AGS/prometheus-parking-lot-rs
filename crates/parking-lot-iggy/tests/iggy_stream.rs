//! Scenario S-33 (cr-33): a client that disconnects mid-stream resumes from the
//! last sequence number it saw and receives every remaining chunk exactly once,
//! then the end marker; a retried or re-published chunk reaches the consumer
//! once.
//!
//! Every test starts a pinned `apache/iggy` container, so each is `#[ignore]`d
//! and compiles in CI without Docker. Run them with:
//!
//! ```text
//! cargo nextest run -p prometheus-parking-lot-iggy --locked --run-ignored all -E "test(iggy_)"
//! ```
//!
//! With `--cfg parking_lot_negative_control="s-33"` the cursor forgets where
//! the client was and tracks no sequence numbers; the reconnect and duplicate
//! tests below must then fail.
//!
//! OA-16 uses a server-side frame cap to abandon exactly one chunk. With
//! `--cfg parking_lot_negative_control="oa-16"` the publisher does not poison
//! the writer, and the real-reader `OA-16:no-publication-past-hole` assertion
//! below must fail with `Gap { expected: 3, got: 4 }`.

mod support;

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use prometheus_parking_lot_iggy::{
    IggyOptions, IggyStreamReader, IggyStreamSink, ResumePoint, RetryPolicy, StreamCursor,
    StreamError, StreamItem, StreamOutcome, partition_for,
};
use support::{IggyServer, TestResult, distinct_by_id, read_topic, read_topic_strict};

const FLUSH: Duration = Duration::from_secs(30);
const STEP: Duration = Duration::from_secs(30);

fn options(server: &IggyServer, stream: &str, partitions: u32) -> IggyOptions {
    IggyOptions::new(server.connection_string.clone())
        .with_stream(stream)
        .with_partitions(partitions)
}

/// Read up to `limit` items (or to the end marker).
async fn take(cursor: &mut StreamCursor<'_>, limit: usize) -> TestResult<Vec<StreamItem>> {
    let mut out = Vec::new();
    while out.len() < limit {
        match tokio::time::timeout(STEP, cursor.next()).await? {
            Ok(Some(item)) => out.push(item),
            Ok(None) => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(out)
}

fn seqs(items: &[StreamItem]) -> Vec<u64> {
    items
        .iter()
        .map(|item| match item {
            StreamItem::Chunk(c) => c.seq,
            StreamItem::End(e) => e.seq,
        })
        .collect()
}

fn token(seq: u64) -> String {
    format!("tok-{seq}")
}

/// OA-16: the real server refuses seq 3 before reading its oversized body.
/// Already queued seqs 4-5 must be suppressed, while another writer continues
/// on the same publisher. Disabling poisoning makes the reader see a gap.
#[tokio::test(flavor = "current_thread")]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_stream_abandoned_chunk_poisons_writer_and_preserves_prefix() -> TestResult<()> {
    let server = IggyServer::start_with_frame_limit(Some("16 KiB")).await?;
    let mut failure_logs = server.failure_logs();
    let run = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let stream = format!("oa16-{}-{run}", std::process::id());
    let topic = format!("chunks-{run}");
    let options = options(&server, &stream, 1)
        .with_stream_topic(&topic)
        .with_queue_capacity(8)
        .with_send_timeout(Duration::from_secs(5))
        .with_retry(RetryPolicy::new(
            3,
            Duration::from_millis(100),
            Duration::from_millis(100),
        ));
    eprintln!(
        "OA-16 fixture: stream={stream}, topic={topic}, frame_limit=16 KiB, failed_seq=3, attempts=3"
    );
    let sink = IggyStreamSink::start(&options)?;
    let mut writer = sink.writer("job-hole");
    for seq in 1..=2 {
        assert_eq!(writer.send(token(seq)).await?, seq);
    }
    assert!(sink.flush(FLUSH).await, "valid prefix drained");
    let before = sink.stats();
    assert_eq!(before.published, 2, "prefix must reach the real server");
    assert_eq!(before.failed, 0, "prefix has no abandoned chunk");

    // 64 KiB is below iggy_common 0.11.0's 64,000,000-byte client payload
    // limit, but above this server's 16 KiB wire-frame cap. The wire decoder
    // rejects seq 3 before accepting its body on every attempt; unlike a
    // paused server, it cannot persist the failed chunk after the test resumes.
    // On this current-thread runtime, these five sends fit in the empty queue
    // without yielding to the worker, so 4-5 are queued before 3 can fail.
    assert_eq!(writer.send("x".repeat(64 * 1024)).await?, 3);
    assert_eq!(writer.send(token(4)).await?, 4);
    assert_eq!(writer.send(token(5)).await?, 5);
    let mut healthy = sink.writer("job-healthy");
    assert_eq!(healthy.send("still connected").await?, 1);
    assert_eq!(healthy.finish(StreamOutcome::Completed).await?, 2);
    let queued = sink.stats();
    assert_eq!(queued.enqueued, 7, "all later chunks already queued");
    assert_eq!(queued.published, before.published, "worker has not run yet");
    assert_eq!(queued.failed, before.failed, "tail queued before failure");
    assert!(sink.flush(FLUSH).await, "fault schedule drained");
    eprintln!("OA-16 publisher after fault: {:?}", sink.stats());

    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.open("job-hole");
    let prefix = take(&mut cursor, 2).await?;
    assert_eq!(seqs(&prefix), [1, 2]);
    for (item, seq) in prefix.iter().zip(1..=2) {
        let StreamItem::Chunk(chunk) = item else {
            return Err("OA-16 prefix unexpectedly ended".into());
        };
        assert_eq!(chunk.data, token(seq));
    }
    // This is deliberately the first poisoning oracle. The oa-16 control
    // must fail here with Gap(3,4), not at a counter or writer-only assertion.
    let after_prefix = tokio::time::timeout(Duration::from_secs(2), cursor.next()).await;
    assert!(
        after_prefix.is_err(),
        "OA-16:no-publication-past-hole: expected only seqs 1-2, got {after_prefix:?}"
    );
    let resume = prefix.last().ok_or("missing prefix")?.resume_point();
    drop(cursor);
    drop(reader);

    assert!(writer.is_poisoned(), "exhausted chunk poisons its writer");
    assert_eq!(writer.send("too late").await, Err(StreamError::Poisoned));
    assert_eq!(
        writer.finish(StreamOutcome::Completed).await,
        Err(StreamError::Poisoned)
    );
    assert_eq!(
        writer.next_seq(),
        6,
        "rejected operations consume no sequence"
    );
    assert_eq!(sink.stats().enqueued, 7, "poison rejects before enqueue");
    let stats = sink.stats();
    assert_eq!(stats.failed, 3, "abandoned seq 3 and suppressed seqs 4-5");
    assert_eq!(
        stats.retried - before.retried,
        2,
        "only seq 3 spent retries"
    );
    assert_eq!(stats.published, 4, "prefix and healthy chunk/end published");
    assert_eq!(stats.dropped, 0, "backpressure never dropped a chunk");

    // Poll errors propagate: an unreadable topic cannot pass the absence check.
    let client = server.client().await?;
    let raw = distinct_by_id(
        tokio::time::timeout(STEP, read_topic_strict(&client, &stream, &topic, 1)).await??,
    );
    let hole_seqs: Vec<_> = raw
        .iter()
        .filter(|message| message.body["key"] == "job-hole")
        .map(|message| message.body["seq"].as_u64())
        .collect();
    assert_eq!(
        hole_seqs,
        [Some(1), Some(2)],
        "no failed chunk or queued tail in log"
    );

    // A fresh client resumes both with and without an offset hint. A poisoned
    // stream remains live at the valid prefix; it has no fabricated end marker.
    let reconnected = IggyStreamReader::connect(&options).await?;
    for point in [
        resume,
        ResumePoint {
            after_seq: 2,
            offset: None,
        },
    ] {
        let mut resumed = reconnected.resume("job-hole", point);
        let tail = tokio::time::timeout(Duration::from_secs(2), resumed.next()).await;
        assert!(tail.is_err(), "poisoned-prefix resume must wait: {tail:?}");
    }
    let mut healthy_cursor = reconnected.open("job-healthy");
    let healthy_items = take(&mut healthy_cursor, usize::MAX).await?;
    assert_eq!(seqs(&healthy_items), [1, 2], "publisher remains usable");
    assert!(matches!(
        healthy_items.last(),
        Some(StreamItem::End(end)) if end.outcome == StreamOutcome::Completed
    ));
    failure_logs.disarm();
    Ok(())
}

/// 200 tokens are streamed while the client reads; it disconnects after token
/// 50, reconnects with a fresh client and its last `(seq, offset)`, and gets
/// tokens 51-200 and the end marker, each once. The same holds without the
/// offset hint, and another stream sharing the partition does not leak in.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_stream_reconnect_midstream_receives_remaining_chunks_once() -> TestResult<()> {
    const TOKENS: u64 = 200;
    let server = IggyServer::start().await?;
    // One partition: both streams share it, so the key filter and the offset
    // hint are both exercised.
    let options = options(&server, "resume", 1);
    let sink = IggyStreamSink::start(&options)?;

    let producer = {
        let sink = sink.clone();
        tokio::spawn(async move {
            let mut writer = sink.writer("job-llm");
            let mut other = sink.writer("job-other");
            for seq in 1..=TOKENS {
                writer.send(token(seq)).await?;
                other.send(format!("other-{seq}")).await?;
                if seq % 20 == 0 {
                    tokio::task::yield_now().await;
                }
            }
            writer.finish(StreamOutcome::Completed).await?;
            other.finish(StreamOutcome::Cancelled).await?;
            Ok::<_, StreamError>(())
        })
    };

    // First connection: tokens 1-50, then it goes away.
    let first = IggyStreamReader::connect(&options).await?;
    let mut cursor = first.open("job-llm");
    let head = take(&mut cursor, 50).await?;
    assert_eq!(seqs(&head), (1..=50).collect::<Vec<_>>());
    let last_seen = head
        .last()
        .map(StreamItem::resume_point)
        .unwrap_or_default();
    assert_eq!(last_seen.after_seq, 50);
    drop(cursor);
    drop(first);

    producer.await??;
    assert!(sink.flush(FLUSH).await, "sink drained");

    // Reconnect with the offset hint, and again with the sequence number alone.
    for point in [
        last_seen,
        ResumePoint {
            after_seq: 50,
            offset: None,
        },
    ] {
        let second = IggyStreamReader::connect(&options).await?;
        let mut cursor = second.resume("job-llm", point);
        let tail = take(&mut cursor, usize::MAX).await?;
        assert_eq!(
            seqs(&tail),
            (51..=TOKENS + 1).collect::<Vec<_>>(),
            "tokens 51-200 then the end marker, no gap, no duplicate"
        );
        for item in &tail[..tail.len() - 1] {
            let StreamItem::Chunk(chunk) = item else {
                return Err("an end marker before the last chunk".into());
            };
            assert_eq!(chunk.data, token(chunk.seq));
        }
        let Some(StreamItem::End(end)) = tail.last() else {
            return Err("no end marker".into());
        };
        assert_eq!(end.outcome, StreamOutcome::Completed);
        assert!(
            tokio::time::timeout(STEP, cursor.next()).await??.is_none(),
            "nothing after the end marker"
        );
    }
    Ok(())
}

/// A chunk published twice reaches the consumer once: a repeat from the same
/// sink is skipped by the publisher's window, and a repeat from a restarted
/// sink lands in the log but the cursor skips it by sequence number.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_stream_retried_chunk_is_delivered_once() -> TestResult<()> {
    let server = IggyServer::start().await?;
    let options = options(&server, "retried", 2);
    let sink = IggyStreamSink::start(&options)?;

    let mut writer = sink.writer("job-retry");
    for seq in 1..=5 {
        writer.send(token(seq)).await?;
    }
    // The same sink re-sends 4 and 5 (a retried publish): same ids, skipped.
    let mut again = sink.writer_from("job-retry", 4);
    again.send(token(4)).await?;
    again.send(token(5)).await?;
    assert!(sink.flush(FLUSH).await);
    assert_eq!(sink.stats().deduplicated, 2, "{:?}", sink.stats());

    // A restarted executor has a fresh sink with no memory: 3-5 land twice.
    let restarted = IggyStreamSink::start(&options)?;
    let mut redo = restarted.writer_from("job-retry", 3);
    for seq in 3..=5 {
        redo.send(token(seq)).await?;
    }
    redo.send(token(6)).await?;
    redo.finish(StreamOutcome::Completed).await?;
    assert!(restarted.flush(FLUSH).await);

    // The log really holds the repeats ...
    let client = server.client().await?;
    let raw = read_topic(&client, "retried", "stream.chunks", 2, 0).await?;
    let in_log = raw
        .iter()
        .filter(|m| m.body["key"] == "job-retry" && m.body["seq"] == 3)
        .count();
    assert_eq!(in_log, 2, "seq 3 is in the log twice");

    // ... and the consumer sees each message once.
    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.open("job-retry");
    let items = take(&mut cursor, usize::MAX).await?;
    assert_eq!(seqs(&items), (1..=7).collect::<Vec<_>>());
    assert_eq!(cursor.duplicates_skipped(), 3);
    Ok(())
}

/// A missing chunk is an error, never a silent hole: a wrong offset hint or a
/// lost publish cannot make the reader skip data.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_stream_missing_chunk_is_reported_as_a_gap() -> TestResult<()> {
    let server = IggyServer::start().await?;
    let options = options(&server, "gap", 1);
    let sink = IggyStreamSink::start(&options)?;
    let mut head = sink.writer("job-gap");
    head.send(token(1)).await?;
    head.send(token(2)).await?;
    // seq 3 never published.
    let mut tail = sink.writer_from("job-gap", 4);
    tail.send(token(4)).await?;
    assert!(sink.flush(FLUSH).await);

    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.open("job-gap");
    assert_eq!(seqs(&take(&mut cursor, 2).await?), [1, 2]);
    let error = tokio::time::timeout(STEP, cursor.next()).await?;
    assert_eq!(
        error.err(),
        Some(StreamError::Gap {
            expected: 3,
            got: 4
        })
    );
    Ok(())
}

/// Create `topic` of `stream` with exactly `partitions` partitions, as an
/// operator (or an earlier deployment) would have.
async fn precreate_topic(
    server: &IggyServer,
    stream: &str,
    topic: &str,
    partitions: u32,
) -> TestResult<()> {
    use iggy::prelude::{DirectConfig, IggyExpiry, MaxTopicSize};
    let client = server.client().await?;
    let producer = client
        .producer(stream, topic)?
        .direct(DirectConfig::builder().build())
        .create_stream_if_not_exists()
        .create_topic_if_not_exists(
            partitions,
            IggyExpiry::NeverExpire,
            MaxTopicSize::ServerDefault,
        )
        .build();
    producer.init().await?;
    drop(producer);
    Ok(())
}

/// The topic already exists with fewer partitions than the options say (or the
/// writer and reader were configured differently): the stream must still be
/// written and read, because both use the topic's real partition count.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_stream_uses_the_topics_real_partition_count() -> TestResult<()> {
    let server = IggyServer::start().await?;
    precreate_topic(&server, "narrow", "stream.chunks", 2).await?;
    // A key whose hash lands outside the 2 real partitions when taken mod 4.
    let key = (0..1000)
        .map(|n| format!("job-{n}"))
        .find(|k| partition_for(k, 4) >= 2)
        .ok_or("no key")?;

    let sink = IggyStreamSink::start(&options(&server, "narrow", 4))?;
    let mut writer = sink.writer(key.clone());
    for seq in 1..=3 {
        writer.send(token(seq)).await?;
    }
    writer.finish(StreamOutcome::Completed).await?;
    assert!(sink.flush(FLUSH).await, "sink drained");
    assert!(!writer.is_poisoned(), "{:?}", sink.stats());
    assert_eq!(sink.stats().failed, 0, "{:?}", sink.stats());

    // The reader is configured with yet another count.
    let reader = IggyStreamReader::connect(&options(&server, "narrow", 8)).await?;
    let mut cursor = reader.open(&key);
    let items = take(&mut cursor, usize::MAX).await?;
    assert_eq!(seqs(&items), vec![1, 2, 3, 4]);
    Ok(())
}

/// An offset hint beyond the end of the stream is not detected as a gap: the
/// cursor has nothing to compare it with and waits. The documentation says so;
/// the sequence number, not the hint, is the safe cursor.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs Docker: starts a pinned apache/iggy container"]
async fn iggy_stream_offset_hint_past_the_end_waits_instead_of_reporting() -> TestResult<()> {
    let server = IggyServer::start().await?;
    let options = options(&server, "hint", 1);
    let sink = IggyStreamSink::start(&options)?;
    let mut writer = sink.writer("job-hint");
    writer.send(token(1)).await?;
    writer.finish(StreamOutcome::Completed).await?;
    assert!(sink.flush(FLUSH).await);

    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.resume(
        "job-hint",
        ResumePoint {
            after_seq: 0,
            offset: Some(1_000),
        },
    );
    let waited = tokio::time::timeout(Duration::from_secs(3), cursor.next()).await;
    assert!(
        waited.is_err(),
        "a hint past the end waits (no Gap, no data): {waited:?}"
    );
    // Without the hint the same stream reads fine.
    let mut cursor = reader.resume("job-hint", ResumePoint::default());
    assert_eq!(seqs(&take(&mut cursor, usize::MAX).await?), vec![1, 2]);
    Ok(())
}
