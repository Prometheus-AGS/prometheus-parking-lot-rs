# prometheus-parking-lot-iggy

Apache Iggy lifecycle events and mailbox notifications for
[`prometheus_parking_lot`](https://crates.io/crates/prometheus_parking_lot), a
resource-aware scheduler for expensive work such as LLM inference. Most
applications reach this crate through the facade's `iggy` feature.

Iggy carries events and notifications only. It is not mailbox storage and not
the work queue: results stay in Postgres or redb, and Iggy carries a pointer
event.

## What it holds

- `IggyHook`: a core `Hook` that publishes every lifecycle event to the
  `lifecycle` topic.
- `IggyNotifier`: a core `Notifier` that publishes `mailbox.ready` after a
  result is stored.
- `IggyStreamSink` and `IggyStreamReader`: publish a task's streamed output as
  numbered chunks and resume a stream from the last sequence number a client
  saw.
- `IggyOptions`, `Publisher` and `PublisherStats`: the bounded background
  publisher. Events are dropped and counted when the server is slow or down;
  the caller never waits on the network.
- `TokioHandleSpawner`, `TokioSleeper`, `RetryPolicy`.

Message ids are deterministic UUIDv5 values, so a retried publish carries the
same id.

## Features

This crate has no feature flags. It depends on tokio and the Iggy client,
pinned to `=0.11.0`.

## Where it fits

Register `IggyHook` with a pool's `HookBus` from `prometheus-parking-lot-core`;
wrap `IggyNotifier` in a `BackgroundNotifier` and give it to a `StorageMailbox`.

## License

MIT. See `LICENSE-MIT`.
