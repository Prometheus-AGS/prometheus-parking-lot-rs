//! Runtime spawning contract.

use std::future::Future;

/// Abstraction for spawning task execution on a runtime.
pub trait Spawn {
    /// Spawn an async task that returns a future.
    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static;
}
