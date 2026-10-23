//! # Prometheus Parking Lot - WASM
//!
//! The `wasm32` implementation of `WorkerPool`: tasks run on the browser event
//! loop (`wasm_bindgen_futures::spawn_local`) instead of OS threads, admitted
//! by the same priority dispatcher and unit-weighted semaphore as the native
//! pool in `prometheus-parking-lot-runtime-tokio`. There is no tokio on this
//! target: execution budgets and result timeouts use the JS `setTimeout`
//! through `gloo-timers`.
//!
//! The implementation is compiled only for `target_arch = "wasm32"`; on every
//! other target this crate is empty, so the items named below (`WorkerPool`,
//! `WasmClock`, `sleep`) are plain text rather than links: they exist only in
//! the `wasm32-unknown-unknown` build (the docs.rs build target).
//!
//! # Limits on `wasm32-unknown-unknown`
//!
//! - **Panics abort the module.** The target is built with `panic=abort` on
//!   stable Rust, so a panicking executor cannot be caught: the module traps
//!   and is unusable afterwards. The panic containment of the native pools
//!   (`TaskError::ExecutorPanicked`, worker respawn) does not apply here.
//!   Executors must return a `TaskError`, never panic.
//! - **No blocking.** There is no blocking `retrieve`; use
//!   `WorkerPool::retrieve_async`.
//! - **Executor futures must be `Send`.** `WorkerExecutor::execute` returns a
//!   `Send` future, and JS timer futures are not `Send`; await `sleep`
//!   instead.
//! - **No durable backend in the browser.** Parked tasks live in memory and
//!   are lost with the page. Durable dispatch (`DurableResourcePool`, the
//!   lease queue and result mailbox of the native runtime) does not apply on
//!   the web target: there is nothing in the browser to make a queue durable
//!   against, and none is offered.
//! - **Clock.** `std::time::Instant` and `SystemTime` panic on this target;
//!   the pool reads `WasmClock` (`Date.now()`).
//!
//! # Parity with the native pools (cr-53)
//!
//! The pool runs the same core dispatcher as the native `WorkerPool`, so the
//! admission rules are the same: weighted units per dimension
//! (`resource_limits`, rejected at submit when any dimension can never fit),
//! priority then FIFO order, bounded skip-ahead with the starvation guard
//! (`WorkerPoolConfig::max_head_skips`; `0` is strict head-of-line), and the
//! same lifecycle events through the core `HookBus` (`with_hook`, `hooks`):
//! `Submitted`, `Queued` if it parks, `Started`, exactly one terminal event.
//! There are no threads, so hooks run inline on the call that causes the
//! transition. What is different is the runtime, not the semantics: no
//! blocking `retrieve`, no panic containment, no durable dispatch.
//!
//! # Building and testing
//!
//! ```text
//! cargo check --target wasm32-unknown-unknown -p prometheus-parking-lot-core -p prometheus-parking-lot-wasm
//! cargo test --target wasm32-unknown-unknown -p prometheus-parking-lot-wasm
//! ```
//!
//! The second needs `wasm-bindgen-test-runner` (from `wasm-bindgen-cli`) at
//! exactly the `wasm-bindgen` version in `Cargo.lock`, and Node.js.

// Everything here runs on one thread, on `spawn_local`, and holds JS handles
// that are not `Send`; only `sleep` and the executor's own futures must be.
#![cfg_attr(
    target_arch = "wasm32",
    expect(
        clippy::future_not_send,
        reason = "single-threaded wasm: local tasks hold non-Send JS timer handles"
    )
)]

#[cfg(target_arch = "wasm32")]
mod clock;
#[cfg(target_arch = "wasm32")]
mod events;
#[cfg(target_arch = "wasm32")]
mod pool;
#[cfg(target_arch = "wasm32")]
mod results;
#[cfg(target_arch = "wasm32")]
mod time;

#[cfg(target_arch = "wasm32")]
pub use clock::WasmClock;
#[cfg(target_arch = "wasm32")]
pub use pool::WorkerPool;
#[cfg(target_arch = "wasm32")]
pub use prometheus_parking_lot_core::{Hook, HookBus, LifecycleEvent, PoolStats, SchedulerError};
#[cfg(target_arch = "wasm32")]
pub use time::sleep;
