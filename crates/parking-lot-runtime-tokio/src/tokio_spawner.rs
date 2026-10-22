//! Tokio runtime spawner implementation.

use std::future::Future;
use std::sync::Arc;

use prometheus_parking_lot_core::Spawn;

/// Tokio-based spawner that executes tasks on a tokio runtime.
///
/// Built with [`new`](Self::new) it spawns onto an existing runtime through
/// its handle. Built with [`with_worker_threads`](Self::with_worker_threads)
/// it owns a dedicated runtime, which stays alive as long as any clone of the
/// spawner does.
#[derive(Clone)]
pub struct TokioSpawner {
    handle: tokio::runtime::Handle,
    /// The owned runtime, if this spawner created one. Shared by clones; the
    /// last clone dropped shuts it down.
    runtime: Option<Arc<OwnedRuntime>>,
}

impl std::fmt::Debug for TokioSpawner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokioSpawner")
            .field("ownsruntime", &self.runtime.is_some())
            .finish_non_exhaustive()
    }
}

/// A runtime owned by a spawner.
///
/// Dropping a tokio `Runtime` from inside an asynchronous context panics, and
/// the last spawner clone may well be dropped by a task running on this very
/// runtime. `shutdown_background` is safe from any context, so the drop uses
/// it: spawned tasks are cancelled at their next await point and the worker
/// threads exit in the background.
struct OwnedRuntime(Option<tokio::runtime::Runtime>);

impl Drop for OwnedRuntime {
    fn drop(&mut self) {
        if let Some(runtime) = self.0.take() {
            runtime.shutdown_background();
        }
    }
}

impl TokioSpawner {
    /// Create a new `TokioSpawner` from a tokio runtime handle.
    ///
    /// The spawner does not keep that runtime alive; its owner must.
    #[must_use]
    pub const fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle,
            runtime: None,
        }
    }

    /// Create a `TokioSpawner` that owns a new multi-threaded runtime with
    /// `worker_threads` worker threads.
    ///
    /// The runtime lives as long as the spawner or any of its clones (0.2
    /// dropped it on return, so every spawn went to a dead runtime).
    ///
    /// # Errors
    ///
    /// Returns the I/O error from the tokio runtime builder.
    pub fn with_worker_threads(worker_threads: usize) -> Result<Self, std::io::Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .enable_all()
            .build()?;
        Ok(Self {
            handle: runtime.handle().clone(),
            runtime: Some(Arc::new(OwnedRuntime(Some(runtime)))),
        })
    }
}

impl Spawn for TokioSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        self.handle.spawn(fut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The spawner's own runtime runs spawned work after the constructor
    /// returned (0.2 dropped the runtime, so this send never happened).
    #[test]
    fn ownedruntime_outlives_the_constructor() {
        let spawner = TokioSpawner::with_worker_threads(1).expect("runtime");
        let (tx, rx) = std::sync::mpsc::channel();
        spawner.spawn(async move {
            tx.send(42).expect("receiver alive");
        });
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(42),
            "spawned task never ran"
        );
    }

    /// Dropping the last clone from a task on the owned runtime itself does
    /// not panic.
    #[test]
    fn last_clone_dropped_inside_theruntime_is_safe() {
        let spawner = TokioSpawner::with_worker_threads(1).expect("runtime");
        let (tx, rx) = std::sync::mpsc::channel();
        let inner = spawner.clone();
        drop(spawner);
        inner.clone().spawn(async move {
            drop(inner);
            tx.send(()).expect("receiver alive");
        });
        assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok());
    }
}
