//! In-memory reference implementation of [`LeaseQueue`].
//!
//! It keeps every rule of the contract (leases, fencing, expiry-driven
//! redelivery, delayed requeue, dead letters, idempotent enqueue,
//! priority-then-enqueue-order) with time read from a [`Clock`], so tests drive
//! lease expiry with [`TestClock`](crate::contracts::TestClock) instead of
//! sleeping. It is not durable: everything is lost with the process.
//!
//! Operations lock one mutex and never await while holding it; `lease` is
//! O(n log n) in the number of entries, which is fine for a reference and for
//! development, not for very deep production queues.

use std::cmp::Reverse;
use std::collections::HashMap;
use std::collections::hash_map::RandomState;
use std::future::Future;
use std::hash::{BuildHasher, Hasher};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;

use crate::contracts::clock::{Clock, SystemClock};
use crate::contracts::error::SchedulerError;
use crate::contracts::lease_queue::{
    Enqueued, LeaseQueue, LeaseToken, LeasedTask, NackAction, QueueDepth, QueueEntryId,
};
use crate::contracts::task::ScheduledTask;

/// Name reported in [`SchedulerError::QueueFull`].
const QUEUE_NAME: &str = "in-memory-lease";

/// Where an entry is in its lifecycle. Times are [`Clock::monotonic`] readings.
#[derive(Debug, Clone, Copy)]
enum EntryState {
    /// Leasable once `not_before` is reached (zero for a fresh entry).
    Ready { not_before: Duration },
    /// Leased under `fence` until `until`; leasable again after that.
    Leased { fence: u64, until: Duration },
}

#[derive(Debug)]
struct Entry<P> {
    task: ScheduledTask<P>,
    /// Enqueue order; the FIFO key within a priority. Never changes.
    order: u64,
    attempts: u32,
    state: EntryState,
}

impl<P> Entry<P> {
    /// True when `lease` may hand this entry out at `now`.
    fn is_leasable(&self, now: Duration) -> bool {
        match self.state {
            EntryState::Ready { not_before } => not_before <= now,
            EntryState::Leased { until, .. } => until <= now,
        }
    }

    /// True when `token`'s fence is this entry's current, unexpired lease.
    fn is_held_by(&self, token: LeaseToken, now: Duration) -> bool {
        match self.state {
            EntryState::Leased { fence, until } => fence == token.fence() && now < until,
            EntryState::Ready { .. } => false,
        }
    }
}

/// A dead-lettered task and the reason it was given up on.
#[derive(Debug, Clone)]
pub struct DeadLetter<P> {
    /// The task as it was enqueued.
    pub task: ScheduledTask<P>,
    /// How many times it was leased.
    pub attempts: u32,
    /// The reason given to [`NackAction::DeadLetter`].
    pub reason: String,
}

#[derive(Debug)]
struct State<P> {
    entries: HashMap<QueueEntryId, Entry<P>>,
    /// Idempotency key → entry still in the queue.
    by_idempotency_key: HashMap<String, QueueEntryId>,
    dead_letters: Vec<DeadLetter<P>>,
    next_id: u64,
    next_order: u64,
    next_fence: u64,
}

impl<P> State<P> {
    /// Remove `token`'s entry if `token` holds it, else `LeaseLost`.
    fn take_held(&mut self, token: LeaseToken, now: Duration) -> Result<Entry<P>, SchedulerError> {
        let held = self
            .entries
            .get(&token.entry())
            .is_some_and(|entry| entry.is_held_by(token, now));
        if !held {
            return Err(token.lost());
        }
        let entry = self
            .entries
            .remove(&token.entry())
            .ok_or_else(|| token.lost())?;
        if let Some(key) = &entry.task.meta.idempotency_key {
            self.by_idempotency_key.remove(key);
        }
        Ok(entry)
    }

    /// The entry `token` holds, else `LeaseLost`.
    fn held_mut(
        &mut self,
        token: LeaseToken,
        now: Duration,
    ) -> Result<&mut Entry<P>, SchedulerError> {
        self.entries
            .get_mut(&token.entry())
            .filter(|entry| entry.is_held_by(token, now))
            .ok_or_else(|| token.lost())
    }

    fn enqueue(
        &mut self,
        task: ScheduledTask<P>,
        max_depth: usize,
    ) -> Result<Enqueued, SchedulerError> {
        if let Some(key) = &task.meta.idempotency_key
            && let Some(&existing) = self.by_idempotency_key.get(key)
        {
            return Ok(Enqueued::Duplicate(existing));
        }
        let live = self.entries.len();
        if live >= max_depth {
            return Err(SchedulerError::queue_full(QUEUE_NAME, live, max_depth));
        }
        let id = QueueEntryId(self.next_id);
        self.next_id += 1;
        let order = self.next_order;
        self.next_order += 1;
        if let Some(key) = &task.meta.idempotency_key {
            self.by_idempotency_key.insert(key.clone(), id);
        }
        self.entries.insert(
            id,
            Entry {
                task,
                order,
                attempts: 0,
                state: EntryState::Ready {
                    not_before: Duration::ZERO,
                },
            },
        );
        Ok(Enqueued::Accepted(id))
    }

    fn lease(&mut self, max: usize, now: Duration, until: Duration) -> Vec<LeasedTask<P>>
    where
        P: Clone,
    {
        let mut candidates: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.is_leasable(now))
            .map(|(&id, entry)| (Reverse(entry.task.meta.priority), entry.order, id))
            .collect();
        candidates.sort_unstable();
        candidates.truncate(max);

        let mut leased = Vec::with_capacity(candidates.len());
        for (_, _, id) in candidates {
            let fence = self.next_fence;
            self.next_fence += 1;
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            entry.attempts = entry.attempts.saturating_add(1);
            entry.state = EntryState::Leased { fence, until };
            leased.push(LeasedTask::new(
                entry.task.clone(),
                LeaseToken::new(id, fence),
                entry.attempts,
            ));
        }
        leased
    }

    fn nack(
        &mut self,
        token: LeaseToken,
        action: NackAction,
        now: Duration,
    ) -> Result<(), SchedulerError> {
        match action {
            NackAction::Requeue { delay } => {
                self.held_mut(token, now)?.state = EntryState::Ready {
                    not_before: now.saturating_add(delay),
                };
            }
            NackAction::DeadLetter { reason } => {
                let entry = self.take_held(token, now)?;
                self.dead_letters.push(DeadLetter {
                    task: entry.task,
                    attempts: entry.attempts,
                    reason,
                });
            }
        }
        Ok(())
    }

    fn extend(
        &mut self,
        token: LeaseToken,
        now: Duration,
        until: Duration,
    ) -> Result<(), SchedulerError> {
        self.held_mut(token, now)?.state = EntryState::Leased {
            fence: token.fence(),
            until,
        };
        Ok(())
    }

    fn depth(&self, now: Duration) -> QueueDepth {
        let (mut ready, mut delayed, mut leased) = (0, 0, 0);
        for entry in self.entries.values() {
            match entry.state {
                _ if entry.is_leasable(now) => ready += 1,
                EntryState::Ready { .. } => delayed += 1,
                EntryState::Leased { .. } => leased += 1,
            }
        }
        QueueDepth::new(ready, delayed, leased, self.dead_letters.len())
    }
}

/// In-memory [`LeaseQueue`] with leases timed by a [`Clock`].
///
/// ```
/// use std::time::Duration;
/// use prometheus_parking_lot_core::contracts::{InMemoryLeaseQueue, LeaseQueue};
///
/// let queue = InMemoryLeaseQueue::<String>::new(100);
/// assert_eq!(queue.max_depth(), 100);
/// # let _ = Duration::ZERO;
/// ```
#[derive(Debug)]
pub struct InMemoryLeaseQueue<P, C = SystemClock> {
    max_depth: usize,
    /// [`LeaseQueue::identity`]: unique to this value.
    identity: String,
    clock: C,
    state: Mutex<State<P>>,
}

impl<P> InMemoryLeaseQueue<P> {
    /// A queue holding at most `max_depth` live entries, timed by the system
    /// clock.
    #[must_use]
    pub fn new(max_depth: usize) -> Self {
        Self::with_clock(max_depth, SystemClock::new())
    }
}

impl<P, C: Clock> InMemoryLeaseQueue<P, C> {
    /// A queue holding at most `max_depth` live entries, timed by `clock`.
    #[must_use]
    pub fn with_clock(max_depth: usize, clock: C) -> Self {
        Self {
            max_depth,
            identity: fresh_identity(),
            clock,
            state: Mutex::new(State {
                entries: HashMap::new(),
                by_idempotency_key: HashMap::new(),
                dead_letters: Vec::new(),
                next_id: 1,
                next_order: 1,
                next_fence: 1,
            }),
        }
    }

    /// The dead letters, oldest first.
    #[must_use]
    pub fn dead_letters(&self) -> Vec<DeadLetter<P>>
    where
        P: Clone,
    {
        self.state.lock().dead_letters.clone()
    }

    fn now(&self) -> Duration {
        self.clock.monotonic()
    }
}

/// A queue identity no other `InMemoryLeaseQueue` has: a per-process random
/// nonce (so a restarted process does not reuse its predecessor's identities
/// against a mailbox that outlived it) and a per-process counter.
///
/// The nonce comes from the standard library's per-process hash keys; on
/// targets without OS randomness (`wasm32-unknown-unknown`) those keys are
/// fixed, so identities there are unique within one instance only.
fn fresh_identity() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    static NONCE: OnceLock<u64> = OnceLock::new();
    let nonce = *NONCE.get_or_init(|| RandomState::new().build_hasher().finish());
    // Relaxed: only uniqueness matters, which the RMW gives on its own.
    let instance = NEXT.fetch_add(1, Ordering::Relaxed);
    format!("in-memory:{nonce:016x}:{instance}")
}

fn require_positive(lease_for: Duration) -> Result<(), SchedulerError> {
    if lease_for.is_zero() {
        return Err(SchedulerError::invalid_config(
            "lease duration must be greater than zero",
        ));
    }
    Ok(())
}

/// Every operation completes when it is called (one lock, no waiting) and
/// returns an already-resolved future.
impl<P, C> LeaseQueue<P> for InMemoryLeaseQueue<P, C>
where
    P: Clone + Send,
    C: Clock,
{
    fn enqueue(
        &self,
        task: ScheduledTask<P>,
    ) -> impl Future<Output = Result<Enqueued, SchedulerError>> + Send {
        std::future::ready(self.state.lock().enqueue(task, self.max_depth))
    }

    fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> impl Future<Output = Result<Vec<LeasedTask<P>>, SchedulerError>> + Send {
        let leased = require_positive(lease_for).map(|()| {
            if max == 0 {
                return Vec::new();
            }
            let now = self.now();
            self.state
                .lock()
                .lease(max, now, now.saturating_add(lease_for))
        });
        std::future::ready(leased)
    }

    fn ack(&self, token: LeaseToken) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        let now = self.now();
        std::future::ready(self.state.lock().take_held(token, now).map(drop))
    }

    fn nack(
        &self,
        token: LeaseToken,
        action: NackAction,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        let now = self.now();
        std::future::ready(self.state.lock().nack(token, action, now))
    }

    fn extend(
        &self,
        token: LeaseToken,
        lease_for: Duration,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        let extended = require_positive(lease_for).and_then(|()| {
            let now = self.now();
            self.state
                .lock()
                .extend(token, now, now.saturating_add(lease_for))
        });
        std::future::ready(extended)
    }

    fn depth(&self) -> impl Future<Output = Result<QueueDepth, SchedulerError>> + Send {
        let now = self.now();
        std::future::ready(Ok(self.state.lock().depth(now)))
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    fn identity(&self) -> &str {
        &self.identity
    }
}
