//! Time source contract.
//!
//! Every time read a pool makes (deadline checks, execution budgets, audit
//! timestamps, queue pruning) goes through a [`Clock`], so tests can drive
//! time deterministically with [`TestClock`] instead of sleeping.
//!
//! A clock provides two readings:
//!
//! - [`Clock::monotonic`]: elapsed time since the clock's own origin. It never
//!   goes backwards and is the reading for measuring intervals (leases, TTL
//!   sweeps, budgets).
//! - [`Clock::now_ms`]: wall-clock milliseconds since the Unix epoch. It is the
//!   unit of [`TaskMetadata::deadline_ms`](crate::contracts::TaskMetadata) and
//!   `created_at_ms`, which travel between processes, so it must be wall time.
//!
//! [`SystemClock`] reads `std::time`. On `wasm32-unknown-unknown` the standard
//! library clocks panic when read; the WASM crate supplies its own clock there
//! (cr-38).

use std::fmt;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// A source of monotonic and wall-clock time.
///
/// Implementations must be cheap to call and safe to share between threads.
pub trait Clock: Send + Sync + fmt::Debug {
    /// Elapsed time since this clock's origin. Never decreases.
    fn monotonic(&self) -> Duration;

    /// Wall-clock time in milliseconds since the Unix epoch.
    fn now_ms(&self) -> u128;
}

impl<C: Clock + ?Sized> Clock for Arc<C> {
    fn monotonic(&self) -> Duration {
        (**self).monotonic()
    }

    fn now_ms(&self) -> u128 {
        (**self).now_ms()
    }
}

/// The process clock: `std::time::Instant` for monotonic time and
/// `std::time::SystemTime` for wall time.
///
/// All `SystemClock` values share one monotonic origin (the first monotonic
/// reading in the process), so readings from different values are comparable.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemClock;

impl SystemClock {
    /// Create the system clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Clock for SystemClock {
    fn monotonic(&self) -> Duration {
        static ORIGIN: OnceLock<Instant> = OnceLock::new();
        ORIGIN.get_or_init(Instant::now).elapsed()
    }

    fn now_ms(&self) -> u128 {
        now_ms()
    }
}

/// A manually driven clock for deterministic tests.
///
/// Clones share the same time: advancing one advances them all, so a test
/// keeps a handle while the pool holds another. Time moves only when the test
/// calls [`advance`](Self::advance) or [`set_wall_ms`](Self::set_wall_ms).
#[derive(Debug, Clone)]
pub struct TestClock {
    state: Arc<Mutex<TestClockState>>,
}

#[derive(Debug)]
struct TestClockState {
    monotonic: Duration,
    wall_ms: u128,
}

impl TestClock {
    /// A clock whose wall time starts at `wall_ms` and whose monotonic reading
    /// starts at zero.
    #[must_use]
    pub fn new(wall_ms: u128) -> Self {
        Self {
            state: Arc::new(Mutex::new(TestClockState {
                monotonic: Duration::ZERO,
                wall_ms,
            })),
        }
    }

    /// Move both readings forward by `by`.
    pub fn advance(&self, by: Duration) {
        let mut state = self.state.lock();
        state.monotonic = state.monotonic.saturating_add(by);
        state.wall_ms = state.wall_ms.saturating_add(by.as_millis());
    }

    /// Set the wall-clock reading without touching monotonic time (models a
    /// wall-clock step such as an NTP correction).
    pub fn set_wall_ms(&self, wall_ms: u128) {
        self.state.lock().wall_ms = wall_ms;
    }
}

impl Default for TestClock {
    /// A clock starting at wall time 0 (the Unix epoch).
    fn default() -> Self {
        Self::new(0)
    }
}

impl Clock for TestClock {
    fn monotonic(&self) -> Duration {
        self.state.lock().monotonic
    }

    fn now_ms(&self) -> u128 {
        self.state.lock().wall_ms
    }
}

/// Returns the system wall-clock time in milliseconds since the Unix epoch.
///
/// Code that tests need to drive should read time through a [`Clock`]; this
/// free function is the system reading for callers building task metadata.
#[must_use]
pub fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clock_moves_only_when_advanced() {
        let clock = TestClock::new(1_000);
        let shared = clock.clone();
        assert_eq!(clock.now_ms(), 1_000);
        assert_eq!(clock.monotonic(), Duration::ZERO);

        shared.advance(Duration::from_millis(250));
        assert_eq!(clock.now_ms(), 1_250);
        assert_eq!(clock.monotonic(), Duration::from_millis(250));

        clock.set_wall_ms(10);
        assert_eq!(shared.now_ms(), 10);
        assert_eq!(shared.monotonic(), Duration::from_millis(250));
    }

    #[test]
    fn system_clock_is_monotonic() {
        let clock = SystemClock::new();
        let a = clock.monotonic();
        let b = clock.monotonic();
        assert!(b >= a);
        assert!(clock.now_ms() > 0);
    }
}
