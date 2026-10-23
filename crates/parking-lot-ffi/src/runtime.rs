//! The tokio runtime that drives every exported future.
//!
//! `UniFFI` runs an async export by polling its future from the foreign runtime
//! (Kotlin coroutines, Swift concurrency), which is not a tokio runtime. The embedded
//! store hands its blocking redb transactions to `tokio::task::spawn_blocking`, which
//! panics outside a tokio context. Every exported method therefore spawns its work
//! onto this crate's own runtime and awaits the join handle, which is runtime
//! agnostic.
//!
//! Spawning also fixes the cancellation semantics: when the foreign caller cancels
//! (drops the future), the spawned work still runs to completion, which is the
//! guarantee the embedded backend documents ("a started transaction is committed or
//! rolled back, never left half done"). A cancelled `enqueue` may still have stored
//! the task; retry with the same idempotency key.

use std::future::Future;
use std::sync::OnceLock;

use tokio::runtime::{Builder, Runtime};

use crate::error::FfiError;

/// Worker threads for the runtime. The work is short coordination around blocking
/// transactions (which run on tokio's blocking pool), so two is enough on a phone.
const WORKER_THREADS: usize = 2;

fn runtime() -> Result<&'static Runtime, FfiError> {
    static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(WORKER_THREADS)
                .thread_name("parking-lot-ffi")
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|message| FfiError::Internal {
            detail: format!("cannot start the parking-lot runtime: {message}"),
        })
}

/// Run `work` to completion on the crate's runtime.
pub async fn run<T, F>(work: F) -> Result<T, FfiError>
where
    T: Send + 'static,
    F: Future<Output = Result<T, FfiError>> + Send + 'static,
{
    runtime()?
        .spawn(work)
        .await
        .map_err(|e| FfiError::Internal {
            detail: format!("parking-lot task failed: {e}"),
        })?
}
