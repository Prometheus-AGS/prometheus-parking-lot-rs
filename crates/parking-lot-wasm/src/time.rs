//! Timers without a runtime.
//!
//! The native pools bound executions with `tokio::time::timeout`. There is no
//! tokio on `wasm32-unknown-unknown`; the browser's `setTimeout` (through
//! `gloo-timers`) is the timer, and the future that waits on it is not `Send`
//! (it holds JS handles). [`timeout`] races a future against such a timer;
//! dropping the race drops the timer, which calls `clearTimeout`.
//!
//! [`sleep`] is the same timer behind a `Send` future, because
//! `WorkerExecutor::execute` must return a `Send` future and a raw
//! `TimeoutFuture` cannot be awaited inside it.

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use futures_channel::oneshot;
use gloo_timers::future::TimeoutFuture;
use wasm_bindgen_futures::spawn_local;

/// The timer fired before the future finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Elapsed;

/// The longest delay `setTimeout` honours: browsers and Node.js store it as a
/// signed 32-bit count and fire a larger one at once (about 24.8 days).
const MAX_TIMER_MS: u32 = i32::MAX.cast_unsigned();

/// A duration as `setTimeout` milliseconds, saturating at [`MAX_TIMER_MS`].
fn timer_millis(duration: Duration) -> u32 {
    u32::try_from(duration.as_millis()).map_or(MAX_TIMER_MS, |ms| ms.min(MAX_TIMER_MS))
}

/// Run `future` for at most `duration`.
///
/// The future is polled before the timer, so a result that is ready at the
/// instant the timer fires is delivered rather than dropped. On elapse the
/// future is dropped, which cancels it at its next await point.
pub async fn timeout<F: Future>(duration: Duration, future: F) -> Result<F::Output, Elapsed> {
    let mut future = pin!(future);
    let mut timer = pin!(TimeoutFuture::new(timer_millis(duration)));
    poll_fn(|cx| {
        if let Poll::Ready(output) = future.as_mut().poll(cx) {
            return Poll::Ready(Ok(output));
        }
        timer.as_mut().poll(cx).map(|()| Err(Elapsed))
    })
    .await
}

/// Wait for `duration` on the browser's timer.
///
/// Unlike a bare `gloo_timers::future::TimeoutFuture`, this future is `Send`,
/// so an executor can await it inside `WorkerExecutor::execute`. Dropping it
/// before the timer fires cancels the timer.
///
/// Must be called on a thread that is running the JS event loop (it spawns a
/// local task).
pub async fn sleep(duration: Duration) {
    let (mut done, waiting) = oneshot::channel::<()>();
    spawn_local(async move {
        let mut timer = pin!(TimeoutFuture::new(timer_millis(duration)));
        let fired = poll_fn(|cx| {
            if timer.as_mut().poll(cx).is_ready() {
                return Poll::Ready(true);
            }
            // The sleeper was dropped: stop waiting and clear the timer.
            if done.poll_canceled(cx).is_ready() {
                return Poll::Ready(false);
            }
            Poll::Pending
        })
        .await;
        if fired {
            let _ = done.send(());
        }
    });
    let _ = waiting.await;
}
