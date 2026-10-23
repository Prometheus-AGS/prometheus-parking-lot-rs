//! Off-path, bounded-retry notification for [`StorageMailbox`](crate::contracts::StorageMailbox).
//!
//! The core crate has no async runtime, so this module needs no timer or
//! executor of its own. Two small seams carry the runtime-dependent parts:
//!
//! - the existing [`Spawn`] contract starts the notification task, so the
//!   delivery never awaits the notifier;
//! - [`Sleeper`] waits out a retry backoff. The tokio crate (or any runtime
//!   adapter) implements it with its own timer; tests implement it with a
//!   recorder that returns immediately.
//!
//! The retry schedule is a pure function, [`RetryPolicy::backoff_after`].
//!
//! # Surfacing failures
//!
//! A failing notifier is reported two ways, neither of which can fail storage:
//! every failed attempt is logged with `tracing` (`warn` while a retry
//! remains, `error` once the budget is spent), and [`NotifyStats`] counts
//! notified, retried and abandoned notifications so an operator or a test can
//! read them without a subscriber. Counters are pull-based and need no
//! runtime, unlike an error stream or a callback that would have to choose a
//! channel or a thread to run on.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use crate::contracts::clock::{Clock, SystemClock};
use crate::contracts::result_mailbox::{Delivery, MailboxEntry, NoopNotifier, Notifier};
use crate::contracts::spawn::Spawn;
use crate::contracts::types::MailboxKey;

/// Waits for a retry backoff. The seam that keeps timers out of core.
pub trait Sleeper: Send + Sync {
    /// Resolve after `duration`.
    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> + Send;
}

/// Bounded retry schedule for a failing notifier: exponential backoff, capped.
///
/// `max_attempts` counts the first try, so `1` means no retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetryPolicy {
    /// Total attempts, including the first. Values below 1 count as 1.
    pub max_attempts: u32,
    /// Wait after the first failed attempt.
    pub initial_backoff: Duration,
    /// Upper bound on any single wait.
    pub max_backoff: Duration,
}

impl RetryPolicy {
    /// A policy with explicit bounds.
    #[must_use]
    pub const fn new(max_attempts: u32, initial_backoff: Duration, max_backoff: Duration) -> Self {
        Self {
            max_attempts,
            initial_backoff,
            max_backoff,
        }
    }

    /// The wait before the next attempt after `attempt` (1-based) failed, or
    /// `None` when the budget is spent. Doubles each time, capped at
    /// `max_backoff`.
    #[must_use]
    pub fn backoff_after(&self, attempt: u32) -> Option<Duration> {
        if attempt >= self.max_attempts.max(1) {
            return None;
        }
        let shift = attempt.saturating_sub(1).min(31);
        let wait = self
            .initial_backoff
            .saturating_mul(1_u32 << shift)
            .min(self.max_backoff);
        Some(wait)
    }
}

impl Default for RetryPolicy {
    /// Five attempts, 100 ms doubling to at most 5 s.
    fn default() -> Self {
        Self::new(5, Duration::from_millis(100), Duration::from_secs(5))
    }
}

/// What a [`BackgroundNotifier`] has done so far.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NotifyStatsSnapshot {
    /// Notifications the notifier accepted.
    pub notified: u64,
    /// Failed attempts that were followed by a retry.
    pub retried: u64,
    /// Notifications given up on after the retry budget was spent.
    pub abandoned: u64,
}

#[derive(Debug, Default)]
struct Counters {
    notified: AtomicU64,
    retried: AtomicU64,
    abandoned: AtomicU64,
}

/// Shared handle to a [`BackgroundNotifier`]'s counters.
#[derive(Debug, Clone, Default)]
pub struct NotifyStats {
    counters: Arc<Counters>,
}

impl NotifyStats {
    /// The counters now.
    #[must_use]
    pub fn snapshot(&self) -> NotifyStatsSnapshot {
        NotifyStatsSnapshot {
            notified: self.counters.notified.load(Ordering::Relaxed),
            retried: self.counters.retried.load(Ordering::Relaxed),
            abandoned: self.counters.abandoned.load(Ordering::Relaxed),
        }
    }
}

/// A notifier a [`StorageMailbox`](crate::contracts::StorageMailbox) can use
/// without waiting for it: `dispatch` returns as soon as the notification is
/// handed off.
///
/// Implemented by [`NoopNotifier`] and [`BackgroundNotifier`]. A bare
/// [`Notifier`] is not accepted, so a slow notifier cannot be awaited on the
/// delivery path by mistake.
pub trait DetachedNotifier<T>: Send + Sync {
    /// Whether `dispatch` does anything; `false` lets the mailbox skip the
    /// copy of the delivery.
    const ACTIVE: bool;

    /// Hand off a notification for the delivery just stored. Never blocks on
    /// the notifier and never fails.
    fn dispatch(&self, delivery: &Delivery<T>);

    /// Notify in line and report the result. Exists only for the S-10
    /// negative control, which notifies before storing.
    #[cfg(parking_lot_negative_control = "s-10")]
    fn notify_inline(
        &self,
        delivery: &Delivery<T>,
    ) -> impl Future<Output = Result<(), crate::contracts::SchedulerError>> + Send;
}

impl<T: Send + Sync> DetachedNotifier<T> for NoopNotifier {
    const ACTIVE: bool = false;

    fn dispatch(&self, _delivery: &Delivery<T>) {}

    #[cfg(parking_lot_negative_control = "s-10")]
    fn notify_inline(
        &self,
        _delivery: &Delivery<T>,
    ) -> impl Future<Output = Result<(), crate::contracts::SchedulerError>> + Send {
        std::future::ready(Ok(()))
    }
}

struct Inner<N, Sl> {
    notifier: N,
    sleeper: Sl,
    counters: Arc<Counters>,
}

impl<N, Sl: Sleeper> Inner<N, Sl> {
    /// Notify under `policy`, the notifier's policy when it was dispatched.
    async fn run<T>(&self, key: MailboxKey, entry: MailboxEntry<T>, policy: RetryPolicy)
    where
        T: Send + Sync,
        N: Notifier<T>,
    {
        let mut attempt = 1_u32;
        loop {
            let Err(error) = self.notifier.notify(&key, &entry).await else {
                self.counters.notified.fetch_add(1, Ordering::Relaxed);
                return;
            };
            if let Some(wait) = policy.backoff_after(attempt) {
                self.counters.retried.fetch_add(1, Ordering::Relaxed);
                tracing::warn!(
                    task_id = entry.task_id,
                    attempt,
                    ?wait,
                    %error,
                    "mailbox notification failed; retrying (the result is stored)"
                );
                self.sleeper.sleep(wait).await;
                attempt += 1;
            } else {
                self.counters.abandoned.fetch_add(1, Ordering::Relaxed);
                tracing::error!(
                    task_id = entry.task_id,
                    attempt,
                    %error,
                    "mailbox notification abandoned after the retry budget (the result is stored)"
                );
                return;
            }
        }
    }
}

/// Runs a [`Notifier`] off the delivery path: each notification is spawned,
/// retried with backoff up to a bound, and its failures counted and logged.
pub struct BackgroundNotifier<N, Sp, Sl> {
    inner: Arc<Inner<N, Sl>>,
    /// Copied into each dispatched notification, so replacing it never
    /// depends on whether a notification still holds `inner`.
    policy: RetryPolicy,
    spawner: Sp,
    clock: Arc<dyn Clock>,
}

impl<N, Sp, Sl> std::fmt::Debug for BackgroundNotifier<N, Sp, Sl> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackgroundNotifier")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl<N, Sp, Sl> BackgroundNotifier<N, Sp, Sl> {
    /// Wrap `notifier`, spawning each notification on `spawner` and waiting
    /// out backoffs with `sleeper`. Uses the default [`RetryPolicy`] and the
    /// [`SystemClock`] for the notified entry's timestamp.
    #[must_use]
    pub fn new(notifier: N, spawner: Sp, sleeper: Sl) -> Self {
        Self {
            inner: Arc::new(Inner {
                notifier,
                sleeper,
                counters: Arc::default(),
            }),
            policy: RetryPolicy::default(),
            spawner,
            clock: Arc::new(SystemClock),
        }
    }

    /// Replace the retry policy. Notifications dispatched from now on use
    /// it; one already in flight keeps the policy it was dispatched with.
    #[must_use]
    pub const fn with_policy(mut self, policy: RetryPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Replace the clock that stamps the notified entry (required on
    /// `wasm32-unknown-unknown`, where [`SystemClock`] cannot be read).
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    /// A handle to this notifier's counters.
    #[must_use]
    pub fn stats(&self) -> NotifyStats {
        NotifyStats {
            counters: Arc::clone(&self.inner.counters),
        }
    }
}

impl<T, N, Sp, Sl> DetachedNotifier<T> for BackgroundNotifier<N, Sp, Sl>
where
    T: Clone + Send + Sync + 'static,
    N: Notifier<T> + 'static,
    Sp: Spawn + Send + Sync,
    Sl: Sleeper + 'static,
{
    const ACTIVE: bool = true;

    fn dispatch(&self, delivery: &Delivery<T>) {
        let key = delivery.key.clone();
        let entry = MailboxEntry::from_delivery(delivery.clone(), self.clock.now_ms());
        let inner = Arc::clone(&self.inner);
        let policy = self.policy;
        self.spawner
            .spawn(async move { inner.run(key, entry, policy).await });
    }

    #[cfg(parking_lot_negative_control = "s-10")]
    fn notify_inline(
        &self,
        delivery: &Delivery<T>,
    ) -> impl Future<Output = Result<(), crate::contracts::SchedulerError>> + Send {
        let entry = MailboxEntry::from_delivery(delivery.clone(), self.clock.now_ms());
        let key = delivery.key.clone();
        let inner = Arc::clone(&self.inner);
        async move { inner.notifier.notify(&key, &entry).await }
    }
}
