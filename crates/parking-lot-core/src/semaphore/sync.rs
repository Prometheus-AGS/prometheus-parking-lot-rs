//! Synchronization primitives for the semaphore and dispatcher.
//!
//! Normal builds use `std` atomics and `Arc` and `parking_lot`'s mutex. Under
//! `--cfg parking_lot_loom` the same names resolve to loom's instrumented
//! types, so the loom models in `tests/loom_*.rs` explore every interleaving of
//! the real dispatcher and semaphore code rather than a copy of it.

#[cfg(not(parking_lot_loom))]
pub use parking_lot::MutexGuard;
#[cfg(not(parking_lot_loom))]
pub use std::sync::Arc;
#[cfg(not(parking_lot_loom))]
pub use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(parking_lot_loom)]
pub use loom::sync::Arc;
#[cfg(parking_lot_loom)]
pub use loom::sync::MutexGuard;
#[cfg(parking_lot_loom)]
pub use loom::sync::atomic::{AtomicU64, Ordering};

/// A non-poisoning mutex: `parking_lot` normally, loom's mutex under loom.
#[derive(Debug)]
pub struct Mutex<T> {
    #[cfg(not(parking_lot_loom))]
    inner: parking_lot::Mutex<T>,
    #[cfg(parking_lot_loom)]
    inner: loom::sync::Mutex<T>,
}

impl<T> Mutex<T> {
    /// Wrap `value`.
    #[cfg_attr(
        not(parking_lot_loom),
        expect(
            clippy::missing_const_for_fn,
            reason = "loom's Mutex::new is not const; one signature for both builds"
        )
    )]
    pub fn new(value: T) -> Self {
        Self {
            #[cfg(not(parking_lot_loom))]
            inner: parking_lot::Mutex::new(value),
            #[cfg(parking_lot_loom)]
            inner: loom::sync::Mutex::new(value),
        }
    }

    /// Lock the mutex. Unlocking (dropping the guard) synchronizes-with the
    /// next `lock`, which is the happens-before edge the dispatcher's
    /// no-lost-wakeup argument rests on.
    pub fn lock(&self) -> MutexGuard<'_, T> {
        #[cfg(not(parking_lot_loom))]
        {
            self.inner.lock()
        }
        #[cfg(parking_lot_loom)]
        {
            // A panic while holding the lock already failed the model; keep
            // going with the inner value rather than masking that failure.
            self.inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
        }
    }
}
