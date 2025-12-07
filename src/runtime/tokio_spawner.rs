//! Tokio runtime spawner implementation.

use std::future::Future;
use std::sync::Arc;

use crate::core::Spawn;

/// Tokio-based spawner that executes tasks on a tokio runtime.
#[derive(Clone)]
pub struct TokioSpawner {
    handle: Arc<tokio::runtime::Handle>,
}

impl TokioSpawner {
    /// Create a new TokioSpawner from a tokio runtime handle.
    pub fn new(handle: tokio::runtime::Handle) -> Self {
        Self {
            handle: Arc::new(handle),
        }
    }

    /// Create a TokioSpawner with a new multi-threaded runtime with specified worker threads.
    pub fn with_worker_threads(worker_threads: usize) -> Result<Self, std::io::Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(worker_threads)
            .enable_all()
            .build()?;
        Ok(Self {
            handle: Arc::new(runtime.handle().clone()),
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
