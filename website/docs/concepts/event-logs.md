---
title: Event logs
description: Append-only event logs from Kafka to Apache Iggy, what they are good for, what they are not, and how the library plans to use Iggy for lifecycle events and notifications.
---

# Event logs

A queue hands each item to one consumer and forgets it. A **log** keeps every
item in order and lets any number of readers move through it at their own pace.
This chapter covers event logs, why they suit lifecycle events and
notifications, and why they do not suit the parking-lot work queue itself.

## The idea

An event log is an append-only, ordered sequence of records split into
partitions. Writers append. Each reader keeps an **offset**: the position of the
next record it wants. Reading does not remove anything; records expire by a
retention policy, usually whole files (segments) at a time.

Three properties follow:

- **Replay.** A reader that was offline resumes from its stored offset and sees
  everything it missed, in order.
- **Fan-out.** Many independent readers (dashboards, audit, a gateway) read the
  same records without coordinating.
- **No per-record state.** The log does not know whether a record was
  "handled". There is no per-message acknowledgement, lease or priority.

```mermaid
flowchart LR
  P["Pool lifecycle events"] -->|append| L
  subgraph L["lifecycle.pool-a"]
    direction LR
    e0["0 Queued"] --- e1["1 Started"] --- e2["2 Completed"] --- e3["3 Queued"] --- e4["4 Started"]
  end
  D["Dashboard<br/>offset 5 (live)"] -.reads.-> e4
  A["Audit<br/>offset 2"] -.reads.-> e2
  G["Gateway, reconnecting<br/>resumes at offset 3"] -.reads.-> e3
```

## History

### Kafka (2011)

Jay Kreps, Neha Narkhede and Jun Rao described Kafka at the NetDB workshop in
2011 as "a distributed messaging system that we developed for collecting and
delivering high volumes of log data with low latency" at LinkedIn. The paper
sets out the model that most event logs now share: a topic is divided into
partitions; each partition is a log stored as a set of segment files; a message
is addressed by its offset rather than an id; consumers pull, and the broker
does not track what each consumer has read. It also notes that a consumer can
deliberately rewind to an old offset and re-consume data, which "violates the
common contract of a queue, but proves to be an essential feature for many
consumers". Retention is time-based: messages are deleted after a configured
period ([Kreps, Narkhede and Rao, 2011](https://www.microsoft.com/en-us/research/wp-content/uploads/2017/09/Kafka.pdf)).

### Apache Iggy

Apache Iggy is a persistent message-streaming platform written in Rust. Its
documentation describes streams that group topics, topics split into
partitions, server-stored consumer offsets, consumer groups that keep ordering
per partition, and retention that expires messages by policy; it supports QUIC,
TCP, WebSocket and HTTP transports
([Iggy documentation](https://iggy.apache.org/docs/introduction/about/)).
Iggy joined the Apache Incubator in February 2025
([Iggy blog, 10 February 2025](https://iggy.apache.org/blogs/2025/02/10/apache-incubator))
and graduated to an Apache top-level project on 19 August 2026. The graduation
post describes it as "built on a thread-per-core design with io_uring for
disk/network I/O"
([Iggy blog, 24 August 2026](https://iggy.apache.org/blogs/2026/08/24/apache-iggy-top-level-project-tlp-graduation/)).

## What a log is good for here, and what it is not

The project's plan of record assessed Iggy against each job the library has.

| Job | Fit | Why |
| --- | --- | --- |
| Lifecycle events (queued, started, completed, failed, expired) | Strong | Append-only, ordered per partition, replayable, many readers |
| "Result ready" notification to a gateway | Strong | A reconnecting gateway resumes from its offset; no webhook retries |
| Streaming-token replay to a reconnecting client | Good | Per-session partition, resume from the last offset received |
| Mailbox storage (keyed read, consume-once, per-entry TTL) | Poor | No key lookup, no per-record delete, segment-level expiry |
| The work queue itself | Poor | No priority, no per-message lease or acknowledgement, no capacity-aware skip-ahead |

So the design keeps results in a store that supports keyed, consume-once reads
(Postgres or an embedded store) and publishes only a **pointer event** to the
log. This is the outbox ordering from [Leases and delivery](./leases.md): store
first, then announce.

```mermaid
sequenceDiagram
  participant Pool
  participant Store as Mailbox storage
  participant Log as Iggy topic mailbox.ready
  participant GW as Gateway
  Pool->>Store: deliver(key, status, result)
  Pool->>Log: publish {key, task_id, status}
  Note over GW: offline
  GW->>Log: reconnect, poll from stored offset
  Log-->>GW: {key, task_id, status}
  GW->>Store: consume(key)
```

## How this library applies it

### What exists on `main`

- `ResourcePool` accepts an `AuditSink` and records `enqueue`, `start`,
  `wake`, `complete`, `fail`, `timeout`, `expire` and `cancel` events through
  it.
- Every native pool (`ResourcePool`, `WorkerPool`, `DurableResourcePool`)
  emits lifecycle events through its `HookBus` (cr-31): `Submitted`, then
  `Queued` if the task parks, `Started` if it runs, and exactly one terminal
  event (`Completed`, `Failed`, `Rejected`, `Cancelled`, `Expired` or
  `Requeued`). Hooks run on a background delivery thread fed by a bounded
  queue, so a slow or panicking hook never stalls dispatch; when the queue is
  full, events are dropped and counted (`HookBus::dropped_events`). With no
  hook registered, emission is one atomic load and a branch. The WASM pool
  does not emit yet (cr-53).
- The place a notifier plugs in exists: `StorageMailbox<S, N>` (cr-34) stores a
  result first and then hands it to a `Notifier` off the delivery path, with
  bounded retries. The only built-in notifier is `NoopNotifier`.

- `cr-32-iggy-events`: behind the `iggy` feature (crate
  `prometheus-parking-lot-iggy`), `IggyHook` publishes every lifecycle event to
  the `lifecycle` topic, partitioned by tenant (or by pool when no tenant is
  set), and `IggyNotifier` publishes `mailbox.ready` after a result is stored.
  Message ids are UUIDv5 values derived from the idempotency key, so a retried
  publish carries the same id. Both publish from a bounded background queue:
  `on_event` only enqueues, and when the server is slow or down the queue fills
  and further events are dropped and counted instead of delaying dispatch.
  Tested against a pinned `apache/iggy:0.9.0` server: a subscriber receives
  `mailbox.ready` for each completed task and replays from an earlier offset.
  Iggy 0.9 deduplicates retried requests per client rather than by message id,
  so the publisher also skips ids it published recently and consumers
  de-duplicate on the id across restarts.

- `cr-33-stream-replay` (stretch): `IggyStreamSink` publishes a task's streamed
  output (for example LLM tokens) as numbered chunks plus an end marker to the
  `stream.chunks` topic, keyed by the idempotency key. A client that
  disconnects mid-stream resumes with `IggyStreamReader::resume` from the last
  sequence number it saw and receives the remaining chunks in order, then live
  ones until the end marker. The sequence number, not the Iggy offset, is the
  cursor: repeats (a retried publish) are skipped and a missing chunk is an
  error, so a resume has no gap and no duplicate. A full publisher queue slows
  the executor instead of dropping chunks.

Iggy is used for events and notifications only, not as mailbox storage and
not as the work queue.

## Sources

1. Jay Kreps, Neha Narkhede, Jun Rao. "Kafka: a Distributed Messaging System for Log Processing." *NetDB 2011*, Athens, 12 June 2011. [PDF](https://www.microsoft.com/en-us/research/wp-content/uploads/2017/09/Kafka.pdf)
2. Apache Iggy. "About." Iggy documentation. [iggy.apache.org](https://iggy.apache.org/docs/introduction/about/)
3. Apache Iggy. "Iggy joins the Apache Incubator." 10 February 2025. [iggy.apache.org](https://iggy.apache.org/blogs/2025/02/10/apache-incubator)
4. Apache Iggy. "Apache Iggy graduates to a Top-Level Project." 24 August 2026. [iggy.apache.org](https://iggy.apache.org/blogs/2026/08/24/apache-iggy-top-level-project-tlp-graduation/)
