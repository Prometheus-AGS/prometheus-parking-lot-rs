//! In-memory reference implementation of [`ResultMailbox`].
//!
//! Keeps every rule of the contract (per-key delivery order, consume-once,
//! first-write-wins remembered past consume, lazy TTL expiry plus eviction)
//! with TTLs timed by a [`Clock`]. Not durable: everything is lost with the
//! process.

use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::time::Duration;

use parking_lot::Mutex;

use crate::contracts::clock::{Clock, SystemClock};
use crate::contracts::error::SchedulerError;
use crate::contracts::result_mailbox::{Delivery, DeliveryOutcome, MailboxEntry, ResultMailbox};
use crate::contracts::types::MailboxKey;

/// A stored entry and its expiry ([`Clock::monotonic`] reading).
#[derive(Debug)]
struct Stored<T> {
    entry: MailboxEntry<T>,
    expires_at: Option<Duration>,
}

/// True when an expiry has passed at `now`.
fn is_expired(expires_at: Option<Duration>, now: Duration) -> bool {
    expires_at.is_some_and(|at| at <= now)
}

#[derive(Debug)]
struct State<T> {
    by_key: HashMap<MailboxKey, VecDeque<Stored<T>>>,
    /// Remembered idempotency keys and when they may be forgotten (the expiry
    /// of the entry they arrived with; `None`: never).
    idempotency_keys: HashMap<String, Option<Duration>>,
}

impl<T> State<T> {
    /// Store `delivery` at monotonic time `now`, wall time `now_ms`.
    fn deliver(&mut self, delivery: Delivery<T>, now: Duration, now_ms: u128) -> DeliveryOutcome {
        let expires_at = delivery.ttl.map(|ttl| now.saturating_add(ttl));
        if let Some(idem) = &delivery.idempotency_key {
            let remembered = self
                .idempotency_keys
                .get(idem)
                .is_some_and(|&forget_at| !is_expired(forget_at, now));
            if remembered {
                return DeliveryOutcome::Duplicate;
            }
            self.idempotency_keys.insert(idem.clone(), expires_at);
        }
        let key = delivery.key.clone();
        let entry = MailboxEntry::from_delivery(delivery, now_ms);
        self.by_key
            .entry(key)
            .or_default()
            .push_back(Stored { entry, expires_at });
        DeliveryOutcome::Stored
    }

    /// Take the oldest unexpired entry for `key`, dropping expired ones.
    fn consume(&mut self, key: &MailboxKey, now: Duration) -> Option<MailboxEntry<T>> {
        let queue = self.by_key.get_mut(key)?;
        queue.retain(|stored| !is_expired(stored.expires_at, now));
        let entry = queue.pop_front().map(|stored| stored.entry);
        if queue.is_empty() {
            self.by_key.remove(key);
        }
        entry
    }

    fn peek(&self, key: &MailboxKey, now: Duration) -> Option<MailboxEntry<T>>
    where
        T: Clone,
    {
        self.by_key
            .get(key)?
            .iter()
            .find_map(|stored| (!is_expired(stored.expires_at, now)).then(|| stored.entry.clone()))
    }

    fn evict_expired(&mut self, now: Duration) -> usize {
        let mut evicted = 0;
        self.by_key.retain(|_, queue| {
            let before = queue.len();
            queue.retain(|stored| !is_expired(stored.expires_at, now));
            evicted += before - queue.len();
            !queue.is_empty()
        });
        self.idempotency_keys
            .retain(|_, &mut forget_at| !is_expired(forget_at, now));
        evicted
    }
}

/// In-memory [`ResultMailbox`] with TTLs timed by a [`Clock`].
#[derive(Debug)]
pub struct InMemoryResultMailbox<T, C = SystemClock> {
    clock: C,
    state: Mutex<State<T>>,
}

impl<T> InMemoryResultMailbox<T> {
    /// An empty mailbox timed by the system clock.
    #[must_use]
    pub fn new() -> Self {
        Self::with_clock(SystemClock::new())
    }
}

impl<T> Default for InMemoryResultMailbox<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, C: Clock> InMemoryResultMailbox<T, C> {
    /// An empty mailbox timed by `clock`.
    #[must_use]
    pub fn with_clock(clock: C) -> Self {
        Self {
            clock,
            state: Mutex::new(State {
                by_key: HashMap::new(),
                idempotency_keys: HashMap::new(),
            }),
        }
    }
}

/// Every operation completes when it is called (one lock, no waiting) and
/// returns an already-resolved future.
impl<T, C> ResultMailbox<T> for InMemoryResultMailbox<T, C>
where
    T: Clone + Send,
    C: Clock,
{
    fn deliver(
        &self,
        delivery: Delivery<T>,
    ) -> impl Future<Output = Result<DeliveryOutcome, SchedulerError>> + Send {
        let (now, now_ms) = (self.clock.monotonic(), self.clock.now_ms());
        std::future::ready(Ok(self.state.lock().deliver(delivery, now, now_ms)))
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send {
        let now = self.clock.monotonic();
        std::future::ready(Ok(self.state.lock().consume(key, now)))
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send {
        let now = self.clock.monotonic();
        std::future::ready(Ok(self.state.lock().peek(key, now)))
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        let now = self.clock.monotonic();
        std::future::ready(Ok(self.state.lock().evict_expired(now)))
    }
}
