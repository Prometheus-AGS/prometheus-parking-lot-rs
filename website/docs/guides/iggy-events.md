---
title: Iggy events
description: Publish every task's lifecycle and a mailbox.ready pointer for each stored result to Apache Iggy, off the dispatch path, so gateways and dashboards react without polling.
---

# Iggy events

**Use it when** something outside the pool must react to tasks: a gateway that
tells a reconnecting client its result is ready, a dashboard of queue health,
an audit trail, or another service that starts when a task completes.

**What you get:** two publishers on Apache Iggy. `IggyHook` publishes every
lifecycle event (`submitted`, `queued`, `started`, then exactly one terminal
event; per run for a durable pool, where `requeued` ends a run that another
follows) to the `lifecycle` topic. `IggyNotifier` publishes a `mailbox.ready`
pointer to the `mailbox.ready` topic after each result is stored. Neither runs
on the dispatch path: a slow or absent server costs bounded buffering and
counted drops, never scheduling latency.

Full source: [`examples/iggy_events.rs`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/examples/iggy_events.rs).
It needs an Iggy 0.9 server (the tests pin `apache/iggy:0.9.0` and client crate
`iggy` 0.11.0):

```bash
docker run -d --name iggy -p 8090:8090 \
  -e IGGY_ROOT_USERNAME=iggy -e IGGY_ROOT_PASSWORD=iggy \
  --cap-add SYS_NICE --security-opt seccomp=unconfined --ulimit memlock=-1:-1 \
  apache/iggy:0.9.0
cargo run --example iggy_events --features "embedded iggy"
```

## Configure the publishers

An `"iggy"` backend section carries the publisher settings. Only
`connection_string` is required (`***` stands for the password; the example
reads the whole string from `IGGY_URL`); the rest are the defaults shown in
[`examples/configs/iggy.json`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/examples/configs/iggy.json):

```json
{
  "type": "iggy",
  "connection_string": "iggy://iggy:***@127.0.0.1:8090",
  "stream": "parking-lot",
  "lifecycle_topic": "lifecycle",
  "mailbox_topic": "mailbox.ready",
  "stream_topic": "stream.chunks",
  "partitions": 4,
  "queue_capacity": 1024,
  "send_timeout_ms": 2000,
  "max_attempts": 5,
  "dedup_window": 4096
}
```

No pool runs on an `"iggy"` backend and the builder opens nothing for it: you
build the hook and the notifier from `IggyBackendConfig::options()`.

```rust title="examples/iggy_events.rs"
    let section: BackendConfig = serde_json::from_value(section)?;
    section.validate("backends.events")?;
    let BackendConfig::Iggy(events) = &section else {
        return Err("not an iggy section".into());
    };
    let options = events.options();
```

## Lifecycle events: a hook

```rust title="examples/iggy_events.rs"
    // Lifecycle events: a hook on the pool's bus.
    let hook = IggyHook::start(&options)?;
    let bus = HookBus::new();
    bus.register(Arc::new(hook.clone()));
```

Pass the bus to a durable pool with `DurableOptions::with_hooks`, register the
hook on an in-memory pool with `with_hook`, or return it from the hooks factory
of `build_pools_from_scheduler_config_with_hooks`. Events are partitioned by the
configured `tenant`, else by pool name, so one tenant's (or pool's) events stay
in order.

## Result notifications: a notifier in the mailbox

A `StorageMailbox` stores the result first, then hands it to the notifier on a
background task with bounded retries. A notifier failure never fails or undoes
the stored result.

```rust title="examples/iggy_events.rs"
    let notifier = IggyNotifier::start(&options)?;
    let dir = tempfile::tempdir()?;
    let store = EmbeddedStore::open(dir.path().join("parking-lot.redb"))?;
    let mailbox = StorageMailbox::new(
        store.result_mailbox::<String>(),
        BackgroundNotifier::new(
            notifier.clone(),
            TokioHandleSpawner::current(),
            TokioSleeper,
        ),
    );
```

`BackgroundNotifier` comes from the facade (`prometheus_parking_lot::core`,
with its `RetryPolicy` and `Sleeper`); `TokioHandleSpawner` and `TokioSleeper`
come from `infra::events`. The pool then runs on that mailbox:

```rust title="examples/iggy_events.rs"
    let pool = DurableResourcePool::start(
        PoolLimits {
            max_units: 2,
            max_queue_depth: 100,
            default_timeout: Duration::from_secs(30),
        },
        store.lease_queue::<String>(100),
        mailbox,
        Reply,
        TokioSpawner::new(tokio::runtime::Handle::current()),
        DurableOptions::new(Duration::from_secs(30))
            .with_name("chat")
            .with_hooks(bus),
    )?;
```

Three tasks later the counters read:

```text
lifecycle: PublisherStats { enqueued: 9, published: 9, dropped: 0, deduplicated: 0, retried: 0, failed: 0 }
mailbox.ready: PublisherStats { enqueued: 3, published: 3, dropped: 0, deduplicated: 0, retried: 1, failed: 0 }
```

## What a subscriber receives

A `mailbox.ready` message is a pointer, not the result: the mailbox key, the
task id, the status, the idempotency key and a `result_ref`. The subscriber
tells the client where to look, and the client reads the result from the
mailbox ([Disconnected clients](./disconnected-clients.md)). Messages are
partitioned by tenant.

Every message id is a UUIDv5 of the task's idempotency key (or of its address
and task id when it has none), so a retried publish carries the same id.
Iggy 0.9 de-duplicates retried requests per client, not by message id: the
publisher skips repeats it remembers (`dedup_window`), and a consumer that must
be exactly-once across publisher restarts de-duplicates on the id.

## What this does not do

- **Lifecycle events can be dropped.** When the server is down long enough to
  fill `queue_capacity`, further events are dropped and counted in
  `stats().dropped`. Use them for observation, not as the record of truth: the
  mailbox and the lease queue are that.
- **A configuration-built pool cannot notify.** `build_pools_from_scheduler_config`
  gives a durable pool a plain mailbox. To publish `mailbox.ready`, start the
  `DurableResourcePool` yourself with a `StorageMailbox`, as above. Hooks work
  either way.
- **In-memory pools have no notifier.** `ResourcePool` delivers to a synchronous
  `Mailbox`; only the durable pool's `ResultMailbox` takes a notifier. An
  in-memory pool can still publish lifecycle events, and its `Completed` event
  serves as the signal.
- **Iggy is not storage here.** It holds events and pointers. Results stay in
  Postgres or redb; the work queue is never Iggy.

Concepts: [Event logs](../concepts/event-logs.md). Streaming output:
[Streaming replay](./streaming-replay.md).
