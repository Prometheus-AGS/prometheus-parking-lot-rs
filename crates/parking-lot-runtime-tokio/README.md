# prometheus-parking-lot-runtime-tokio

The tokio runtime layer of [`prometheus_parking_lot`](https://crates.io/crates/prometheus_parking_lot),
a resource-aware scheduler for expensive work such as LLM inference. Most
applications depend on the `prometheus_parking_lot` facade, which re-exports
this crate behind its default `tokio-runtime` feature.

It builds on the contracts in `prometheus-parking-lot-core`.

## What it holds

- `ResourcePool`: the async pool. It admits a task when its cost fits the free
  capacity, parks it otherwise, and delivers each outcome to a mailbox.
- `DurableResourcePool` and `DurableOptions`: the crash-durable mode, which
  parks tasks in a `LeaseQueue` and delivers results to a `ResultMailbox`.
- `WorkerPool` (native targets only): executors on dedicated OS threads, each
  with its own single-threaded tokio runtime.
- `ShutdownHandle`, `ShutdownPolicy`, `ShutdownReport`, `wait_for_signal`:
  graceful shutdown.
- `CancellationSink`, `CancellationChannel` and the logging and recording
  sinks: capture of cancelled tasks.
- `TokioSpawner`: the tokio implementation of the core `Spawn` contract.

## Features

This crate has no feature flags. It requires a tokio runtime (`rt-multi-thread`).

## Where it fits

Backends for the durable pool come from `prometheus-parking-lot-pg` and
`prometheus-parking-lot-embedded`. On `wasm32-unknown-unknown` use
`prometheus-parking-lot-wasm` instead; this crate does not build there.

## License

MIT. See `LICENSE-MIT`.
