//! The browser clock.
//!
//! `std::time::Instant::now` and `SystemTime::now` panic on
//! `wasm32-unknown-unknown` (the standard library has no clock there), so the
//! core's `SystemClock` cannot be used. [`WasmClock`] reads `Date.now()`
//! through `js-sys`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use prometheus_parking_lot_core::Clock;

/// A [`Clock`] backed by JavaScript's `Date.now()`.
///
/// `now_ms` is wall-clock milliseconds since the Unix epoch, the unit of
/// `TaskMetadata::deadline_ms`. `monotonic` is the elapsed time since the
/// clock was created, held non-decreasing even when the system clock steps
/// back. Resolution is one millisecond (`Date.now()`).
#[derive(Debug)]
pub struct WasmClock {
    /// `Date.now()` when the clock was created.
    origin_ms: u64,
    /// Largest elapsed reading handed out so far.
    last_elapsed_ms: AtomicU64,
}

impl WasmClock {
    /// Create a clock whose monotonic origin is now.
    #[must_use]
    pub fn new() -> Self {
        Self {
            origin_ms: date_now_ms(),
            last_elapsed_ms: AtomicU64::new(0),
        }
    }
}

impl Default for WasmClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for WasmClock {
    fn monotonic(&self) -> Duration {
        let elapsed = date_now_ms().saturating_sub(self.origin_ms);
        // fetch_max returns the previous maximum; the reading is the larger of
        // the two, so a backwards step of the system clock never shows.
        // Relaxed: a single value with no other data published through it.
        let previous = self.last_elapsed_ms.fetch_max(elapsed, Ordering::Relaxed);
        Duration::from_millis(previous.max(elapsed))
    }

    fn now_ms(&self) -> u128 {
        u128::from(date_now_ms())
    }
}

/// `Date.now()` as whole milliseconds since the Unix epoch.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "Date.now() is a finite, non-negative millisecond count far below u64::MAX"
)]
fn date_now_ms() -> u64 {
    js_sys::Date::now().max(0.0) as u64
}
