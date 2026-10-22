//! Clock utilities placeholder.

/// Returns a wall-clock timestamp in milliseconds since the Unix epoch.
#[must_use]
pub fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}
