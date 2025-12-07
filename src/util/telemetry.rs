//! Telemetry helpers for structured logging and tracing.

/// Initialize tracing/telemetry. Users can install their own subscriber; this
/// helper installs a default env-based subscriber if none is set.
pub fn init_tracing() {
    if tracing::dispatcher::has_been_set() {
        return;
    }
    let _ = tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .try_init();
}
