# prometheus-parking-lot-wasm

The `wasm32-unknown-unknown` `WorkerPool` for
[`prometheus_parking_lot`](https://crates.io/crates/prometheus_parking_lot), a
resource-aware scheduler for expensive work. Tasks run on the browser or Node
event loop (`spawn_local`) instead of OS threads, admitted by the same
dispatcher and unit-weighted semaphore as the native pool. Execution budgets use
JS timers through `gloo-timers`.

On any other target this crate is empty: its items exist only when
`target_arch = "wasm32"`.

## What it holds

- `WorkerPool`: weighted admission per resource dimension, priority then FIFO
  order, bounded skip-ahead, and the same lifecycle events as the native pool.
- `WasmClock`: reads `Date.now()`; `std::time::Instant` panics on this target.
- `sleep`: a JS-timer sleep for executors.
- `Hook`, `HookBus`, `LifecycleEvent`, `PoolStats`, `SchedulerError`:
  re-exported from `prometheus-parking-lot-core`.

## Limits

- A panic aborts the module (`panic=abort`); executors must return a `TaskError`.
- No blocking `retrieve`; use `WorkerPool::retrieve_async`.
- No durable backend: parked tasks are lost with the page.

## Features

This crate has no feature flags.

## Using it

Through the facade, depend on `prometheus_parking_lot` with
`default-features = false` on `wasm32-unknown-unknown`; it pulls this crate in
by target.

## License

MIT. See `LICENSE-MIT`.
