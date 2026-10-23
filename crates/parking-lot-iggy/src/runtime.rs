//! Tokio implementations of the core crate's runtime seams, so
//! [`BackgroundNotifier`](prometheus_parking_lot_core::mailbox::BackgroundNotifier)
//! can wrap an [`IggyNotifier`](crate::IggyNotifier).

use std::future::Future;
use std::time::Duration;

use prometheus_parking_lot_core::Spawn;
use prometheus_parking_lot_core::mailbox::Sleeper;
use tokio::runtime::Handle;

/// Spawns notification tasks on a Tokio runtime.
#[derive(Debug, Clone)]
pub struct TokioHandleSpawner(Handle);

impl TokioHandleSpawner {
    /// Spawn on `handle`'s runtime.
    #[must_use]
    pub const fn new(handle: Handle) -> Self {
        Self(handle)
    }

    /// Spawn on the runtime the caller is running in.
    ///
    /// # Panics
    ///
    /// Outside a Tokio runtime.
    #[must_use]
    #[track_caller]
    pub fn current() -> Self {
        Self(Handle::current())
    }
}

impl Spawn for TokioHandleSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        drop(self.0.spawn(fut));
    }
}

/// Waits out retry backoffs with Tokio's timer.
#[derive(Debug, Clone, Copy, Default)]
pub struct TokioSleeper;

impl Sleeper for TokioSleeper {
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(duration)
    }
}
