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

## Browser and Node assurance

OA-13 adds the same real scheduler/JS-timer suites to headless Chromium while
preserving wasm-bindgen's Node default. The checked
[manifest](../../.github/wasm-browser-assurance.json) names 15 pool and 6 event
tests. The [boundary runner](../../.github/scripts/tests/browser-assurance.py)
requires exact nonzero discovery, complete execution and an actual in-WASM
browser Document/user-agent marker; Node-only results cannot satisfy browser
evidence. A source-derived control must detect a missing timeout terminal event.

After the complete phase implementation set, from the workspace root with a
clean frozen candidate and fresh external output paths:

```bash
python3 .github/scripts/tests/browser-assurance.py --mode node --toolchain 1.99.1 \
  --output "$OA13_NODE_OUT"
python3 .github/scripts/tests/browser-assurance.py --mode browser \
  --chrome "$APPROVED_CHROME" --chromedriver "$APPROVED_CHROMEDRIVER" \
  --output "$OA13_BROWSER_OUT"
```

Browser provisioning is separate: the authored path requires Linux x86_64,
preinstalled matching Chrome/ChromeDriver `156.0.8078.47`, Rust `1.99.1` with the
WASM target and wasm-bindgen runner `0.2.129`; Node lanes require Node 24 and
also retain the `1.95` MSRV leg. No browser installation or execution is claimed.
All OA-13 runtime results remain NOT_EXERCISED until the final boundary. This
in-memory browser pool offers no durable page-loss recovery or panic containment.

## License

MIT. See `LICENSE-MIT`.
