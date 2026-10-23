---
title: Streaming replay
description: Stream a task's output token by token through Apache Iggy, and let a client that disconnected mid-stream resume with no gap and no repeat.
---

# Streaming replay

**Use it when** a task produces output incrementally, such as LLM tokens, and
the client should see it as it is produced, but the client's connection may
drop before the task ends.

**What you get:** the executor publishes each piece as a numbered chunk with an
`IggyStreamSink`, plus an end marker. A client keeps the sequence number of the
last chunk it saw; after reconnecting it gets exactly the chunks after that
one, in order, then live ones until the end marker. A missing chunk is an
error, never a silent hole.

Full source: [`examples/iggy_stream_replay.rs`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/examples/iggy_stream_replay.rs).
It needs an Iggy 0.9 server (see [Iggy events](./iggy-events.md) for the
`docker run` line):

```bash
cargo run --example iggy_stream_replay --features iggy
```

## The executor streams

The stream key is the task's idempotency key: the client already holds it.

```rust title="examples/iggy_stream_replay.rs"
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
```

`send` never drops a chunk: when the publisher's queue is full it waits, which
slows the executor to the publisher's pace. That wait is in the executor's own
task, not on the scheduler's dispatch path. If a chunk is abandoned after its
retries, the writer is poisoned and refuses every later chunk, so the stream
cannot continue past a hole.

The task's ordinary result still goes to its mailbox; the stream is in
addition to it.

## The client resumes

```rust title="examples/iggy_stream_replay.rs"
    // First connection: read five tokens, remember where we were, go away.
    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.open(&stream_key);
    let head = read(&mut cursor, 5).await?;
    let saved: ResumePoint = head
        .last()
        .map(StreamItem::resume_point)
        .unwrap_or_default();
```

```rust title="examples/iggy_stream_replay.rs"
    let reader = IggyStreamReader::connect(&options).await?;
    let mut cursor = reader.resume(&stream_key, saved);
    let tail = read(&mut cursor, usize::MAX).await?;
```

```text
first connection got 5 chunks, resume after seq 5
end marker at seq 21: Completed
second connection got chunks Some(6)..=Some(20)
mailbox: Some((Completed, Some(20)))
```

A `ResumePoint` is the last sequence number seen plus an offset hint. Store it
on the client (local storage, the app's state). The sequence number is the
cursor; the offset only saves re-reading the partition. A wrong offset cannot
silently skip data once later messages arrive: the cursor delivers `last + 1`
and nothing else, skips any lower number as a duplicate, and reports a higher
one as `StreamError::Gap`. A hint that is too high is not detected on its own,
though: if no message follows it (for example the stream already ended), `next()`
waits instead of reporting an error. Take the hint only from
`StreamItem::resume_point`, or omit it, and wrap `next()` in a timeout.

## What this does not do

- **Wrap `next()` in a timeout.** A stream whose writer was poisoned, or whose
  executor died, never gets an end marker; `next()` keeps waiting for live
  chunks. The example's `read` helper times out after 30 s.
- **End the stream on failure yourself.** The example returns early on an
  error without an end marker. A production executor should call
  `finish(StreamOutcome::Failed { .. })` before returning an error.
- **A re-run can splice two generations.** If the task runs again (after a
  crash, at-least-once), its writer starts at sequence 1 again. A client that
  had read past the crash point skips the repeats and continues with the second
  run's chunks, which for a non-deterministic model are not the continuation of
  what it saw. Treat a stream from a re-run as a restart: compare against the
  final result in the mailbox, or have the executor continue with
  `writer_from(key, next_seq)` from output it persisted.

Concepts: [Event logs](../concepts/event-logs.md).
