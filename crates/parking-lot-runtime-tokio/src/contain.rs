//! Panic containment for the native pools (cr-21, scenario S-3).
//!
//! A pool runs user code: the executor, and around it the pool's own
//! completion path. [`contain`] wraps a future so a panic while it is polled,
//! or while it is dropped after panicking, comes back as a [`Panicked`] value
//! instead of unwinding into the pool. The pools use it in two layers:
//!
//! 1. **Executor containment.** Each pool wraps the executor call, its future
//!    and the budget timer in one `contain`, so the call itself (a hand-written
//!    executor can panic before returning its future), every poll, and the
//!    drop of the executor future when the budget elapses are all caught. The
//!    panic becomes [`TaskError::ExecutorPanicked`]; the task's permit, held
//!    outside the contained future, is then returned through
//!    `Dispatcher::complete` like any other outcome, which starts the parked
//!    work the freed units admit.
//! 2. **Backstop.** A panic can still get past layer 1 — a panic payload whose
//!    own destructor panics, or a panic in the pool's completion path (a user
//!    `Clock`, `Mailbox` or `AuditSink`). The pools catch that too: the task's
//!    permit is then freed by unwinding (it is RAII), so the pool calls
//!    `Dispatcher::poke` to start the parked work it admits, reports the task
//!    as panicked if no outcome was recorded yet, and — in the native
//!    `WorkerPool` — replaces the worker thread.
//!
//! ## Unwind safety
//!
//! `catch_unwind` needs `UnwindSafe`; both layers assert it. What the pool can
//! observe after a caught panic is:
//!
//! - the panicking future, which is dropped at once and never polled again;
//! - the shared executor `&E` — the same exposure `tokio::spawn` accepts for
//!   every task (a panic is reported as `JoinError::is_panic` and the runtime
//!   keeps going). An executor that keeps state across tasks owns that
//!   state's consistency, as it would under tokio;
//! - the pool's own state: the semaphore is one atomic changed only by single
//!   read-modify-writes, so it is consistent at every instruction; the
//!   dispatch lock, result slots and mailbox are `parking_lot` mutexes, which
//!   release on unwind without poisoning, and none of their critical sections
//!   calls user code that could unwind half-way through an update (the
//!   `ResourcePool`'s parked queue is a user `TaskQueue`; a backend that
//!   panics inside its own operations is outside this guarantee).
//!
//! ## Negative control
//!
//! Under the test-only `--cfg parking_lot_negative_control="s-3"`, [`catch`]
//! (and so [`contain`]) lets panics through, which disables both layers. The
//! `panic_*` tests must then fail.

use std::any::Any;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use prometheus_parking_lot_core::TaskError;

/// A caught panic: the payload's message, when it was a string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panicked {
    /// The panic message, or a placeholder for a non-string payload.
    pub message: String,
}

impl Panicked {
    /// Describe `payload` and drop it.
    ///
    /// Dropping the payload runs its destructor. A payload whose destructor
    /// panics unwinds out of here; that is the one way a panic in the task
    /// path gets past layer 1, and the backstop catches it.
    fn from_payload(payload: Box<dyn Any + Send>) -> Self {
        let message = payload
            .downcast_ref::<&'static str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "non-string panic payload".to_owned());
        drop(payload);
        Self { message }
    }
}

impl fmt::Display for Panicked {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<Panicked> for TaskError {
    fn from(panicked: Panicked) -> Self {
        Self::ExecutorPanicked {
            message: panicked.message,
        }
    }
}

/// Run `f`, returning a panic as [`Panicked`].
#[cfg(not(parking_lot_negative_control = "s-3"))]
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, Panicked> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).map_err(Panicked::from_payload)
}

/// Negative control S-3: panics are not caught; both containment layers are
/// off.
#[cfg(parking_lot_negative_control = "s-3")]
#[expect(
    clippy::unnecessary_wraps,
    reason = "same signature as the containing build"
)]
pub fn catch<R>(f: impl FnOnce() -> R) -> Result<R, Panicked> {
    Ok(f())
}

/// Wrap `future` so a panic while polling it resolves to `Err(Panicked)`.
pub fn contain<F: Future>(future: F) -> Contained<F> {
    Contained {
        inner: Some(Box::pin(future)),
    }
}

/// The future returned by [`contain`].
///
/// The wrapped future is boxed so `Contained` is `Unpin` without unsafe pin
/// projection (`unsafe_code` is denied). The executor future it usually holds
/// is already a box from `async_trait`, so this is one more small allocation
/// per task.
pub struct Contained<F> {
    /// `None` once the future completed or panicked.
    inner: Option<Pin<Box<F>>>,
}

impl<F: Future> Future for Contained<F> {
    type Output = Result<F::Output, Panicked>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = &mut *self;
        let Some(inner) = this.inner.as_mut() else {
            panic!("`Contained` polled after it completed");
        };
        match catch(|| inner.as_mut().poll(cx)) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(output)) => {
                // A completed async future has already dropped its locals.
                this.inner = None;
                Poll::Ready(Ok(output))
            }
            Err(panicked) => {
                // Drop the panicked future now: whatever it still owns (a
                // permit, a channel end) is released before the caller acts
                // on the panic. A second panic from that drop is swallowed
                // and its payload leaked rather than dropped, since dropping
                // it could panic again; the first panic is the one reported.
                let discarded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    this.inner = None;
                }));
                if let Err(second) = discarded {
                    std::mem::forget(second);
                }
                Poll::Ready(Err(panicked))
            }
        }
    }
}

#[cfg(all(test, not(parking_lot_negative_control = "s-3")))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn contained_future_reports_its_panic_message() {
        let caught = contain(async {
            panic!("boom");
        })
        .await;
        assert_eq!(
            caught,
            Err(Panicked {
                message: "boom".into()
            })
        );
        let formatted = contain(async {
            panic!("task {}", 7);
        })
        .await;
        assert_eq!(
            formatted.map_err(|panicked| panicked.message),
            Err("task 7".into())
        );
        assert_eq!(contain(async { 5 }).await, Ok(5));
    }

    #[test]
    fn non_string_payloads_get_a_placeholder() {
        let caught = catch(|| std::panic::panic_any(42_u32));
        assert_eq!(
            caught.map_err(|panicked| panicked.message),
            Err("non-string panic payload".into())
        );
    }
}
