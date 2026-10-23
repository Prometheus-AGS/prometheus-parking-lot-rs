//! The priority dispatcher: one owner for the parked queue and the
//! unit-weighted semaphore.
//!
//! ## Protocol
//!
//! Every decision about parked work is made under one mutex (the *dispatch
//! lock*), which guards the parked queue and the sequence counter:
//!
//! - **submit**: stamp the sequence, then either start the item (queue empty
//!   and it fits), reject it (queue at depth), or park it and run the drain.
//!   The depth check and the enqueue are one critical section, so the depth
//!   limit holds exactly (no TOCTOU) and a park cannot interleave with a drain.
//! - **drain**: while the head exists: an expired head is popped and reported;
//!   a head that fits is popped and started; a head that does not fit stays
//!   where it is, and the drain tries to *skip ahead* of it (below). The drain
//!   stops when neither the head nor a skip-ahead candidate can start. The
//!   head is never popped to be looked at and pushed back (audit C4).
//! - **complete**: return the finished item's permit *under the dispatch
//!   lock*, then drain.
//!
//! The dispatcher never runs anything: started items come back to the caller
//! as [`Grant`]s (item plus [`Permit`]) to spawn outside the lock.
//!
//! ## Lifecycle events (cr-31)
//!
//! [`submit_with`](Dispatcher::submit_with) calls its `on_park` observer
//! inside the park's critical section, after the enqueue and before the
//! drain. Anything that could start the parked item (this drain, or a
//! completion's) runs after it under the same lock, so a "parked" event the
//! observer emits is ordered before the item's "started" event. The pools
//! emit through a non-blocking `HookBus`, so the observer adds no wait under
//! the lock.
//!
//! ## Skip-ahead and the starvation guard (cr-20)
//!
//! Strict head-of-line order leaves units idle while a large head waits for
//! them. When the head does not fit, the drain instead starts the first
//! parked item, in priority-then-FIFO order, that is not expired and fits the
//! free units (and a free permit): among the items that *can* start, priority
//! and FIFO order still hold. Each such start is a *skip* of the head.
//!
//! The *starvation guard* bounds the skips. The dispatcher counts the skips of
//! the current head, keyed by its sequence number (the count restarts when a
//! different item becomes the head). Once the head has been skipped
//! `max_head_skips` times the dispatcher *reserves* the capacity for it: it
//! starts nothing else until the head has started, so the units that running
//! work returns accumulate for the head. `max_head_skips == 0` turns
//! skip-ahead off (strict head-of-line order). The default is
//! [`DEFAULT_MAX_HEAD_SKIPS`]; see there for the choice.
//!
//! A queue that does not implement [`ParkedQueue::fitting`] keeps strict
//! order whatever `max_head_skips` says.
//!
//! ### The bound
//!
//! Let an item `H` become the head, and suppose it stays the head until it
//! starts: no item that must be served before it (a strictly higher priority)
//! is parked meanwhile, and it is not expired, pruned or taken. Write
//! `N = max_head_skips`, `R` for the items running when `H` became the head
//! and `S` for the items started while `H` is the head and has not started.
//!
//! 1. **`|S| <= N`.** `H` is among the first `N + 1` items the dispatcher
//!    starts after it became the head: it starts within `N` dispatch cycles,
//!    counting one cycle per started item.
//! 2. **`H` starts no later than the drain that follows the last completion
//!    among `R` and `S`.** In wall-clock terms its wait is covered by the run
//!    times of at most `|R| + N` items: at most `(|R| + N) * B` when no item
//!    runs longer than `B`, and in practice far less, as they run in
//!    parallel.
//!
//! *Proof.* (1) Every start happens under the dispatch lock, in the fast path
//! of [`submit`](Dispatcher::submit) or in the drain. The fast path starts an
//! item only when nothing is parked, and `H` is parked. In the drain, an item
//! other than the head starts only as a skip, which requires the head's skip
//! count to be below `N` and increments it. The count is keyed by `H`'s
//! sequence and restarts only when the head's sequence differs, which would
//! contradict `H` staying the head. So at most `N` items start in place of
//! `H`.
//!
//! (2) While `H` is the head, every item holding a permit is in `R` or `S`
//! (by (1), nothing else starts). `H`'s cost is at most `max_units` (larger
//! items are rejected at submission), so `H` fits as soon as no permit is
//! held. Every completion returns its permit under the dispatch lock and then
//! drains ([`complete`](Dispatcher::complete); a permit dropped any other way
//! is followed by [`poke`](Dispatcher::poke) in the pools), and that drain
//! reads `H` as the head and starts it once it fits; so `H` starts no later
//! than the drain after the last completion in `R` and `S`. For the
//! wall-clock form: `H` does not fit only while some permit is held, so from
//! the moment `H` became the head until it starts, some item of `R` or `S` is
//! running at every instant, and that interval is no longer than the sum of
//! their run times. The pools bound a run time by the execution budget where
//! they can stop the executor (the async pools drop its future); a native
//! worker running blocking code holds its units until it returns. ∎
//!
//! The bound is per head. A strictly higher-priority arrival overtakes `H` as
//! it would under strict order, and is then bounded the same way; `H`'s count
//! restarts when it is the head again.
//!
//! Skips and reservations are counted in [`SkipAheadStats`] rather than
//! emitted as lifecycle events: the cr-31 event sequence (`Submitted`,
//! `Queued`, `Started`, one terminal event) has no place for them.
//!
//! Negative control S-20: under
//! `--cfg parking_lot_negative_control="s-20"` the guard never engages
//! (unbounded skip-ahead), and the `starvation_` tests must then find a head
//! that starves.
//!
//! ## Capacity invariant
//!
//! Units are claimed only through [`UnitSemaphore::try_acquire`], whose CAS
//! never moves the count past `max_units` (see `crate::semaphore`). A started
//! item holds a [`Permit`] until it completes, so the units of all running
//! items never exceed `max_units`.
//!
//! ## No lost wakeup
//!
//! Claim: when `complete` returns, no parked head that fits the free capacity
//! is left parked. A parked item becomes startable only when units are
//! returned (a `complete`) or when it reaches the head (a pop, or a park of a
//! higher-priority item; both happen inside a drain-ending critical section).
//! Both `complete` and the park branch of `submit` hold the dispatch lock while
//! they change the state *and* while they drain, so these critical sections
//! are totally ordered by the lock. Take any park P and any completion C:
//!
//! - P before C: the unlock at the end of P synchronizes-with C's lock, so C's
//!   drain sees the parked item and the returned units.
//! - C before P: C's release is sequenced before C's unlock, which
//!   synchronizes-with P's lock; P's own drain therefore observes the returned
//!   units (`try_acquire` reads the latest value in modification order that
//!   happens-before it, or later).
//!
//! The happens-before edge is the mutex unlock → lock pair; it does not depend
//! on the semaphore's own orderings. The `loom_wakeup` model checks this, and
//! fails under `--cfg parking_lot_negative_control="s-2"`, which restores the
//! shape of `main@90a6194`: the admission decision and the enqueue in separate
//! critical sections, with no drain after the enqueue.

use std::marker::PhantomData;

use crate::contracts::SchedulerError;
use crate::semaphore::sync::{Arc, Mutex};
use crate::semaphore::{Permit, UnitSemaphore};

use super::parked::{Parkable, ParkedQueue, PriorityQueue, Ticket};

/// Default for [`Dispatcher::max_head_skips`]: a blocked head may be skipped
/// 8 times before the dispatcher reserves capacity for it.
///
/// The guard trades head latency for utilization. A skip only ever starts
/// work that fits *now*, so the cost to the head is the run time of the
/// skipped work that still holds units when the head could otherwise have
/// started: with `k` small items running side by side, roughly `N / k`
/// small-item run times on top of what strict order would make it wait (see
/// the module docs for the exact bound). Eight skips let the small work that
/// fits beside a waiting head fill its gap for a couple of turnovers, which is
/// where the utilization gain comes from, while keeping the added head wait to
/// a few small-item run times. On the mixed workload of
/// `benches/skip_ahead_bench.rs` (16 units, about 95% offered load) 8 skips
/// reach a utilization of 0.947 against 0.917 for strict order and 0.949
/// unbounded; 2 and 4 skips reach 0.932 and 0.945.
pub const DEFAULT_MAX_HEAD_SKIPS: u32 = 8;

/// Skip-ahead counters of a [`Dispatcher`] (cr-20), since it was built.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkipAheadStats {
    /// Items started in place of a head that did not fit.
    pub skipped: u64,
    /// Heads for which the starvation guard engaged: they were skipped
    /// `max_head_skips` times and the dispatcher then reserved capacity for
    /// them.
    pub reservations: u64,
}

/// The head that is being skipped, and how often.
#[derive(Debug, Clone, Copy)]
struct BlockedHead {
    /// The head's sequence number (unique per dispatcher).
    sequence: u64,
    /// Items started in its place so far.
    skips: u32,
    /// Whether the guard has engaged for it (counted once).
    reserved: bool,
}

/// Whether a head skipped `skips` times may not be skipped again.
#[cfg(not(parking_lot_negative_control = "s-20"))]
const fn guard_engaged(skips: u32, max_head_skips: u32) -> bool {
    skips >= max_head_skips
}

/// Negative control S-20: the starvation guard never engages, so skip-ahead
/// is unbounded and a large head can wait forever behind a stream of small
/// items.
#[cfg(parking_lot_negative_control = "s-20")]
const fn guard_engaged(skips: u32, max_head_skips: u32) -> bool {
    let _ = (skips, max_head_skips);
    false
}

/// An admitted item and the permit it runs under.
#[derive(Debug)]
pub struct Grant<T> {
    /// The item to start.
    pub item: T,
    /// Its claim on the semaphore. Return it with
    /// [`Dispatcher::complete`] when the item finishes.
    pub permit: Permit,
}

/// What a dispatcher operation took off the parked queue.
#[derive(Debug)]
#[must_use = "started items must be spawned and expired items reported"]
pub struct Dispatched<T> {
    /// Items admitted, in dispatch order. The caller must start every one.
    pub started: Vec<Grant<T>>,
    /// Items whose deadline passed while parked. They never ran; the caller
    /// reports them (for example `TaskStatus::Expired`).
    pub expired: Vec<T>,
}

impl<T> Default for Dispatched<T> {
    fn default() -> Self {
        Self {
            started: Vec::new(),
            expired: Vec::new(),
        }
    }
}

impl<T> Dispatched<T> {
    /// True when nothing was started or expired.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.started.is_empty() && self.expired.is_empty()
    }
}

/// The result of an accepted submission.
#[derive(Debug)]
#[must_use = "started items must be spawned and expired items reported"]
pub struct Admission<T> {
    /// The sequence number stamped on the submitted item.
    pub sequence: u64,
    /// Whether the submitted item itself is among `dispatched.started`
    /// (otherwise it is parked).
    pub started: bool,
    /// Everything the submission started or expired, including the
    /// submitted item when it started.
    pub dispatched: Dispatched<T>,
}

/// State guarded by the dispatch lock.
#[derive(Debug)]
struct State<Q> {
    queue: Q,
    /// Last sequence stamped; the next item gets `last_sequence + 1`, so the
    /// first is 1 and 0 stays "unsequenced".
    last_sequence: u64,
    /// Skips allowed per head before capacity is reserved for it; 0 turns
    /// skip-ahead off.
    max_head_skips: u32,
    /// The current head, while it does not fit.
    blocked: Option<BlockedHead>,
    stats: SkipAheadStats,
}

/// Priority dispatcher over a unit-weighted semaphore. See the module docs.
#[derive(Debug)]
pub struct Dispatcher<T, Q = PriorityQueue<T>> {
    pool: String,
    semaphore: Arc<UnitSemaphore>,
    max_queue_depth: usize,
    state: Mutex<State<Q>>,
    _items: PhantomData<fn(T) -> T>,
}

impl<T: Parkable> Dispatcher<T, PriorityQueue<T>> {
    /// A dispatcher with an in-memory parked queue.
    pub fn in_memory(
        pool: impl Into<String>,
        semaphore: UnitSemaphore,
        max_queue_depth: usize,
    ) -> Self {
        Self::new(pool, semaphore, max_queue_depth, PriorityQueue::new())
    }
}

impl<T, Q> Dispatcher<T, Q> {
    /// A dispatcher named `pool` (used in errors) admitting through
    /// `semaphore` and parking at most `max_queue_depth` items in `queue`.
    pub fn new(
        pool: impl Into<String>,
        semaphore: UnitSemaphore,
        max_queue_depth: usize,
        queue: Q,
    ) -> Self {
        Self {
            pool: pool.into(),
            semaphore: Arc::new(semaphore),
            max_queue_depth,
            state: Mutex::new(State {
                queue,
                last_sequence: 0,
                max_head_skips: DEFAULT_MAX_HEAD_SKIPS,
                blocked: None,
                stats: SkipAheadStats::default(),
            }),
            _items: PhantomData,
        }
    }

    /// Set how many times a head that does not fit may be skipped before
    /// capacity is reserved for it (cr-20; see the module docs). `0` turns
    /// skip-ahead off: strict head-of-line order. Default:
    /// [`DEFAULT_MAX_HEAD_SKIPS`].
    #[must_use]
    pub fn with_max_head_skips(self, max_head_skips: u32) -> Self {
        self.set_max_head_skips(max_head_skips);
        self
    }

    /// [`with_max_head_skips`](Self::with_max_head_skips) on a shared
    /// dispatcher. Takes effect at the next drain; a head already skipped at
    /// least `max_head_skips` times gets its reservation at once.
    pub fn set_max_head_skips(&self, max_head_skips: u32) {
        self.state.lock().max_head_skips = max_head_skips;
    }

    /// Skips allowed per head before capacity is reserved for it.
    #[must_use]
    pub fn max_head_skips(&self) -> u32 {
        self.state.lock().max_head_skips
    }

    /// Skip-ahead counters since the dispatcher was built.
    #[must_use]
    pub fn skip_ahead_stats(&self) -> SkipAheadStats {
        self.state.lock().stats
    }

    /// Total units.
    #[must_use]
    pub fn max_units(&self) -> u32 {
        self.semaphore.max_units()
    }

    /// Units held by started, not yet completed items.
    #[must_use]
    pub fn units_in_use(&self) -> u32 {
        self.semaphore.units_in_use()
    }

    /// Started, not yet completed items.
    #[must_use]
    pub fn running(&self) -> u32 {
        self.semaphore.permits_outstanding()
    }
}

impl<T: Parkable, Q: ParkedQueue<T>> Dispatcher<T, Q> {
    /// Items currently parked.
    #[must_use]
    pub fn parked(&self) -> usize {
        self.state.lock().queue.len()
    }

    /// Remove and return every parked item (for shutdown), in dispatch order.
    ///
    /// Items leave through the same peek-then-pop path as a drain, so a
    /// [`TaskQueueAdapter`](super::TaskQueueAdapter) hands back what its
    /// backend holds too. If the queue's head becomes unreadable, the items
    /// taken so far are returned and the rest stay parked (logged); a caller
    /// that counts accepted tasks sees them as not settled.
    pub fn take_parked(&self) -> Vec<T> {
        let mut state = self.state.lock();
        let mut taken = Vec::new();
        loop {
            match state.queue.head() {
                Ok(Some(_)) => match state.queue.pop_head() {
                    Some(item) => taken.push(item),
                    None => break,
                },
                Ok(None) => break,
                Err(error) => {
                    tracing::error!(
                        pool = %self.pool,
                        %error,
                        "parked queue head unreadable; items left parked"
                    );
                    break;
                }
            }
        }
        taken
    }

    /// Submit `item`: start it, park it, or reject it.
    ///
    /// The dispatcher stamps the item's sequence. The item starts at once only
    /// when nothing is parked and it fits; otherwise it is parked, and the
    /// drain that follows may start it (or other items) right away.
    ///
    /// # Errors
    ///
    /// - [`SchedulerError::CapacityExceeded`] when the item needs more units
    ///   than the semaphore has in total (it could never start);
    /// - [`SchedulerError::QueueFull`] when it must wait and `max_queue_depth`
    ///   items are already parked;
    /// - the parked queue's error when it cannot store the item.
    ///
    /// On error the item is dropped and nothing else changes (its sequence
    /// number stays unused).
    pub fn submit(&self, item: T, now_ms: u128) -> Result<Admission<T>, SchedulerError> {
        self.submit_with(item, now_ms, |_| {})
    }

    /// [`submit`](Self::submit), calling `on_park(depth)` when the item is
    /// parked, with `depth` the number of parked items including it.
    ///
    /// `on_park` runs under the dispatch lock, after the enqueue and before
    /// the drain that may start the item; see the module docs. It must not
    /// block or call back into the dispatcher.
    ///
    /// # Errors
    ///
    /// As [`submit`](Self::submit); `on_park` is not called then.
    pub fn submit_with(
        &self,
        mut item: T,
        now_ms: u128,
        on_park: impl FnOnce(usize),
    ) -> Result<Admission<T>, SchedulerError> {
        let units = item.ticket().units;
        if units > self.semaphore.max_units() {
            return Err(SchedulerError::CapacityExceeded {
                pool: self.pool.clone(),
                requested: units,
                available: self.semaphore.max_units(),
            });
        }

        let mut state = self.state.lock();
        state.last_sequence += 1;
        let sequence = state.last_sequence;
        item.set_sequence(sequence);

        if state.queue.is_empty()
            && let Some(permit) = UnitSemaphore::try_acquire(&self.semaphore, units)
        {
            drop(state);
            return Ok(Admission {
                sequence,
                started: true,
                dispatched: Dispatched {
                    started: vec![Grant { item, permit }],
                    expired: Vec::new(),
                },
            });
        }

        let depth = state.queue.len();
        if depth >= self.max_queue_depth {
            return Err(SchedulerError::queue_full(
                self.pool.clone(),
                depth,
                self.max_queue_depth,
            ));
        }

        #[cfg(parking_lot_negative_control = "s-2")]
        {
            // Negative control S-2 (main@90a6194): the admission decision and
            // the enqueue are separate critical sections, and the enqueue does
            // not drain. A completion between the two finds an empty queue,
            // and the item is then parked with nothing left to wake it.
            drop(state);
            let mut state = self.state.lock();
            state.queue.push(item)?;
            on_park(state.queue.len());
            drop(state);
            return Ok(Admission {
                sequence,
                started: false,
                dispatched: Dispatched::default(),
            });
        }

        #[cfg(not(parking_lot_negative_control = "s-2"))]
        {
            state.queue.push(item)?;
            on_park(state.queue.len());
            let dispatched = self.drain_locked(&mut state, now_ms);
            drop(state);
            let started = dispatched
                .started
                .iter()
                .any(|grant| grant.item.ticket().sequence == sequence);
            Ok(Admission {
                sequence,
                started,
                dispatched,
            })
        }
    }

    /// Return a finished item's permit and take whatever the freed capacity
    /// admits.
    ///
    /// The permit is released under the dispatch lock; see the module docs
    /// for why that rules out a lost wakeup.
    pub fn complete(&self, permit: Permit, now_ms: u128) -> Dispatched<T> {
        let mut state = self.state.lock();
        drop(permit);
        let dispatched = self.drain_locked(&mut state, now_ms);
        drop(state);
        dispatched
    }

    /// Run the drain on its own: start every head that fits and report
    /// expired heads. Pools call it after anything that may unblock the head
    /// outside `submit`/`complete` (for example a permit dropped by an
    /// unwinding task).
    pub fn poke(&self, now_ms: u128) -> Dispatched<T> {
        let mut state = self.state.lock();
        let dispatched = self.drain_locked(&mut state, now_ms);
        drop(state);
        dispatched
    }

    /// Remove parked items whose deadline passed at `now_ms`, then drain
    /// (removing a head that did not fit can unblock the items behind it).
    ///
    /// # Errors
    ///
    /// The parked queue's error when pruning fails; nothing is drained then.
    pub fn prune_expired(&self, now_ms: u128) -> Result<(usize, Dispatched<T>), SchedulerError> {
        let mut state = self.state.lock();
        let removed = state.queue.prune_expired(now_ms)?;
        let dispatched = self.drain_locked(&mut state, now_ms);
        drop(state);
        Ok((removed, dispatched))
    }

    /// The drain. Callers hold the dispatch lock.
    ///
    /// Peek-then-pop: the head is removed only once it is known to be expired
    /// or admitted, so a head that does not fit never leaves the queue.
    fn drain_locked(&self, state: &mut State<Q>, now_ms: u128) -> Dispatched<T> {
        let mut dispatched = Dispatched::default();
        loop {
            let ticket = match state.queue.head() {
                Ok(Some(ticket)) => ticket,
                Ok(None) => break,
                Err(error) => {
                    // Nothing was removed; the next submit/complete retries.
                    tracing::error!(pool = %self.pool, %error, "parked queue head unreadable");
                    break;
                }
            };
            if ticket.is_expired(now_ms) {
                if let Some(item) = state.queue.pop_head() {
                    dispatched.expired.push(item);
                }
                continue;
            }
            if let Some(permit) = UnitSemaphore::try_acquire(&self.semaphore, ticket.units) {
                match state.queue.pop_head() {
                    Some(item) => dispatched.started.push(Grant { item, permit }),
                    None => break,
                }
                continue;
            }
            if !self.skip_ahead_locked(state, &ticket, now_ms, &mut dispatched) {
                break;
            }
        }
        dispatched
    }

    /// One skip of `head`, which does not fit: start the first parked item
    /// that does, unless the starvation guard reserves capacity for the head.
    /// Returns whether an item started. Callers hold the dispatch lock.
    ///
    /// The candidate is admitted before it is removed, so an item whose
    /// permit cannot be had stays parked (as in the head path).
    fn skip_ahead_locked(
        &self,
        state: &mut State<Q>,
        head: &Ticket,
        now_ms: u128,
        dispatched: &mut Dispatched<T>,
    ) -> bool {
        if state.max_head_skips == 0 {
            return false;
        }
        let mut blocked = match state.blocked {
            Some(blocked) if blocked.sequence == head.sequence => blocked,
            _ => BlockedHead {
                sequence: head.sequence,
                skips: 0,
                reserved: false,
            },
        };
        if guard_engaged(blocked.skips, state.max_head_skips) {
            if !blocked.reserved {
                blocked.reserved = true;
                state.stats.reservations += 1;
                tracing::debug!(
                    pool = %self.pool,
                    sequence = head.sequence,
                    skips = blocked.skips,
                    "starvation guard: reserving capacity for the head"
                );
            }
            state.blocked = Some(blocked);
            return false;
        }
        state.blocked = Some(blocked);
        let Some(free_units) = self.semaphore.available_units() else {
            return false;
        };
        let candidate = match state.queue.fitting(free_units, now_ms) {
            Ok(Some(candidate)) => candidate,
            Ok(None) => return false,
            Err(error) => {
                // Nothing was removed; the next submit/complete retries.
                tracing::error!(pool = %self.pool, %error, "parked queue unreadable for skip-ahead");
                return false;
            }
        };
        let Some(permit) = UnitSemaphore::try_acquire(&self.semaphore, candidate.units) else {
            return false;
        };
        // Dropping `permit` on the `None` path returns its units under the
        // dispatch lock.
        let Some(item) = state.queue.take(&candidate) else {
            return false;
        };
        if candidate != *head {
            // A permit dropped outside the lock can make the head itself the
            // candidate; that start is not a skip.
            blocked.skips = blocked.skips.saturating_add(1);
            state.blocked = Some(blocked);
            state.stats.skipped += 1;
        }
        dispatched.started.push(Grant { item, permit });
        true
    }
}

#[cfg(all(test, not(parking_lot_loom)))]
mod tests {
    use super::super::parked::Ticket;
    use super::*;
    use crate::contracts::Priority;

    #[derive(Debug)]
    struct Item {
        name: &'static str,
        priority: Priority,
        units: u32,
        deadline_ms: Option<u128>,
        sequence: u64,
    }

    const fn item(name: &'static str, priority: Priority, units: u32) -> Item {
        Item {
            name,
            priority,
            units,
            deadline_ms: None,
            sequence: 0,
        }
    }

    impl Parkable for Item {
        fn ticket(&self) -> Ticket {
            Ticket {
                priority: self.priority,
                sequence: self.sequence,
                units: self.units,
                deadline_ms: self.deadline_ms,
            }
        }

        fn set_sequence(&mut self, sequence: u64) {
            self.sequence = sequence;
        }
    }

    fn names(grants: &[Grant<Item>]) -> Vec<&'static str> {
        grants.iter().map(|grant| grant.item.name).collect()
    }

    #[test]
    fn starts_parks_and_wakes_in_priority_then_fifo_order() {
        let dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(4), 8);
        let first = dispatcher
            .submit(item("first", Priority::Low, 4), 0)
            .expect("accepted");
        assert!(first.started);
        let mut running = first.dispatched.started;

        for (name, priority) in [
            ("low", Priority::Low),
            ("high-1", Priority::High),
            ("high-2", Priority::High),
        ] {
            let admission = dispatcher
                .submit(item(name, priority, 2), 0)
                .expect("accepted");
            assert!(!admission.started);
            assert!(admission.dispatched.is_empty());
        }
        assert_eq!(dispatcher.parked(), 3);

        let woken = dispatcher.complete(running.pop().expect("first").permit, 0);
        assert_eq!(names(&woken.started), vec!["high-1", "high-2"]);
        assert_eq!(dispatcher.units_in_use(), 4);
        assert_eq!(dispatcher.parked(), 1);
    }

    #[test]
    fn strict_order_head_that_does_not_fit_blocks_and_stays_parked() {
        let dispatcher =
            Dispatcher::in_memory("test", UnitSemaphore::new(4), 8).with_max_head_skips(0);
        let blocker = dispatcher
            .submit(item("blocker", Priority::Normal, 2), 0)
            .expect("ok");
        let big = dispatcher
            .submit(item("big", Priority::High, 4), 0)
            .expect("ok");
        assert!(!big.started);
        // Skip-ahead off: the small task waits behind the big head even
        // though it would fit.
        let small = dispatcher
            .submit(item("small", Priority::Low, 1), 0)
            .expect("ok");
        assert!(!small.started);
        assert_eq!(dispatcher.parked(), 2);
        let mut grants = blocker.dispatched.started;
        let woken = dispatcher.complete(grants.pop().expect("blocker").permit, 0);
        assert_eq!(names(&woken.started), vec!["big"]);
        assert_eq!(dispatcher.skip_ahead_stats(), SkipAheadStats::default());
    }

    /// The head that does not fit stays parked while a smaller item skips
    /// ahead of it; the head starts as soon as it fits.
    #[test]
    fn starvation_skip_ahead_starts_a_fitting_item_behind_the_head() {
        let dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(4), 8);
        assert_eq!(dispatcher.max_head_skips(), DEFAULT_MAX_HEAD_SKIPS);
        let blocker = dispatcher
            .submit(item("blocker", Priority::Normal, 2), 0)
            .expect("ok");
        let big = dispatcher
            .submit(item("big", Priority::High, 4), 0)
            .expect("ok");
        assert!(!big.started);
        let small = dispatcher
            .submit(item("small", Priority::Low, 1), 0)
            .expect("ok");
        assert!(small.started, "the small item fits beside the blocker");
        assert_eq!(dispatcher.parked(), 1);
        assert_eq!(dispatcher.units_in_use(), 3);
        let mut running = blocker.dispatched.started;
        running.extend(small.dispatched.started);
        let mut woken = Vec::new();
        for grant in running {
            woken.extend(dispatcher.complete(grant.permit, 0).started);
        }
        assert_eq!(names(&woken), vec!["big"]);
        assert_eq!(dispatcher.skip_ahead_stats().skipped, 1);
    }

    /// Among the items behind the head, the ones that fit start in priority,
    /// then FIFO, order.
    #[test]
    fn starvation_skip_ahead_keeps_priority_order_among_fitting_items() {
        let dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(6), 16);
        let x1 = dispatcher
            .submit(item("x1", Priority::Normal, 4), 0)
            .expect("ok");
        let x2 = dispatcher
            .submit(item("x2", Priority::Normal, 2), 0)
            .expect("ok");
        for (name, priority, units) in [
            ("head", Priority::Critical, 5),
            ("normal-3", Priority::Normal, 3),
            ("low-1", Priority::Low, 1),
            ("normal-1a", Priority::Normal, 1),
            ("high-1", Priority::High, 1),
            ("normal-1b", Priority::Normal, 1),
        ] {
            let admission = dispatcher
                .submit(item(name, priority, units), 0)
                .expect("ok");
            assert!(!admission.started, "nothing fits while x1 and x2 run");
        }
        let mut grants = x1.dispatched.started;
        let woken = dispatcher.complete(grants.pop().expect("x1").permit, 0);
        // 4 units free, the head needs 5: the fitting items start in
        // priority-then-FIFO order until the units run out.
        assert_eq!(names(&woken.started), vec!["high-1", "normal-3"]);
        let mut grants = x2.dispatched.started;
        let woken = dispatcher.complete(grants.pop().expect("x2").permit, 0);
        assert_eq!(names(&woken.started), vec!["normal-1a", "normal-1b"]);
        assert_eq!(dispatcher.parked(), 2, "the head and low-1 wait");
        assert_eq!(dispatcher.skip_ahead_stats().skipped, 4);
    }

    /// Skips are taken in priority-then-FIFO order among the fitting items,
    /// passing over one that does not fit.
    #[test]
    fn starvation_skip_ahead_order_passes_over_items_that_do_not_fit() {
        let dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(4), 16);
        let blocker = dispatcher
            .submit(item("blocker", Priority::Normal, 2), 0)
            .expect("ok");
        let _ = dispatcher
            .submit(item("head", Priority::Critical, 4), 0)
            .expect("ok");
        let mut started = Vec::new();
        for (name, priority, units) in [
            ("normal-2", Priority::Normal, 3),
            ("low-1", Priority::Low, 1),
            ("high-1", Priority::High, 1),
        ] {
            let admission = dispatcher
                .submit(item(name, priority, units), 0)
                .expect("ok");
            started.extend(names(&admission.dispatched.started));
        }
        // normal-2 (3 units) never fits beside the blocker; low-1 then
        // high-1 fit, each skipping ahead as it arrives.
        assert_eq!(started, vec!["low-1", "high-1"]);
        assert_eq!(dispatcher.parked(), 2);
        drop(blocker);
    }

    /// After `max_head_skips` skips the guard reserves capacity for the head:
    /// items that fit keep waiting until the head has started.
    #[test]
    fn starvation_guard_reserves_capacity_after_max_skips() {
        let dispatcher =
            Dispatcher::in_memory("test", UnitSemaphore::new(4), 16).with_max_head_skips(2);
        let blocker = dispatcher
            .submit(item("blocker", Priority::Normal, 1), 0)
            .expect("ok");
        let _ = dispatcher
            .submit(item("head", Priority::Normal, 4), 0)
            .expect("ok");
        let mut started = Vec::new();
        for name in ["s1", "s2", "s3"] {
            let admission = dispatcher
                .submit(item(name, Priority::Normal, 1), 0)
                .expect("ok");
            started.extend(admission.dispatched.started);
        }
        assert_eq!(names(&started), vec!["s1", "s2"]);
        let stats = dispatcher.skip_ahead_stats();
        assert_eq!((stats.skipped, stats.reservations), (2, 1));
        // One unit is free, s3 fits, but the head is reserved.
        assert_eq!(dispatcher.units_in_use(), 3);
        assert_eq!(dispatcher.parked(), 2);
        let mut woken = Vec::new();
        let mut running = blocker.dispatched.started;
        running.extend(started);
        for grant in running {
            woken.extend(dispatcher.complete(grant.permit, 0).started);
        }
        assert_eq!(names(&woken), vec!["head"]);
        let mut running = woken;
        let after = dispatcher.complete(running.pop().expect("head").permit, 0);
        assert_eq!(names(&after.started), vec!["s3"]);
    }

    #[test]
    fn take_parked_returns_every_parked_item_in_dispatch_order() {
        let dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(1), 8);
        let running = dispatcher
            .submit(item("running", Priority::Normal, 1), 0)
            .expect("ok");
        for (name, priority) in [("low", Priority::Low), ("high", Priority::High)] {
            let _ = dispatcher.submit(item(name, priority, 1), 0).expect("ok");
        }
        let taken: Vec<_> = dispatcher
            .take_parked()
            .into_iter()
            .map(|item| item.name)
            .collect();
        assert_eq!(taken, vec!["high", "low"]);
        assert_eq!(dispatcher.parked(), 0);
        drop(running);
    }

    #[test]
    fn depth_limit_and_oversized_items_are_rejected() {
        let dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(2), 1);
        let running = dispatcher
            .submit(item("a", Priority::Normal, 2), 0)
            .expect("ok");
        let parked = dispatcher
            .submit(item("b", Priority::Normal, 1), 0)
            .expect("ok");
        assert!(!parked.started);
        assert!(matches!(
            dispatcher.submit(item("c", Priority::Normal, 1), 0),
            Err(SchedulerError::QueueFull { depth: 1, .. })
        ));
        assert!(matches!(
            dispatcher.submit(item("d", Priority::Normal, 3), 0),
            Err(SchedulerError::CapacityExceeded { requested: 3, .. })
        ));
        drop(running);
    }

    #[test]
    fn expired_heads_are_reported_not_started() {
        let dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(1), 8);
        let running = dispatcher
            .submit(item("a", Priority::Normal, 1), 0)
            .expect("ok");
        let mut late = item("late", Priority::High, 1);
        late.deadline_ms = Some(5);
        let _ = dispatcher.submit(late, 0).expect("ok");
        let _ = dispatcher
            .submit(item("b", Priority::Normal, 1), 0)
            .expect("ok");
        let mut grants = running.dispatched.started;
        let woken = dispatcher.complete(grants.pop().expect("a").permit, 10);
        assert_eq!(woken.expired.len(), 1);
        assert_eq!(names(&woken.started), vec!["b"]);
    }

    /// Threads submit and complete concurrently; every accepted item runs
    /// exactly once and capacity is never exceeded. Also the miri target for
    /// the dispatcher's lock + atomics path.
    #[test]
    fn concurrent_submit_and_complete_lose_nothing() {
        use std::sync::atomic::{AtomicU32, Ordering};
        const THREADS: u32 = 4;
        let per_thread: u32 = if cfg!(miri) { 8 } else { 500 };
        let dispatcher = std::sync::Arc::new(Dispatcher::in_memory(
            "test",
            UnitSemaphore::new(3),
            usize::MAX,
        ));
        let held = std::sync::Arc::new(AtomicU32::new(0));
        let ran = std::sync::Arc::new(AtomicU32::new(0));
        let handles: Vec<_> = (0..THREADS)
            .map(|thread| {
                let dispatcher = std::sync::Arc::clone(&dispatcher);
                let held = std::sync::Arc::clone(&held);
                let ran = std::sync::Arc::clone(&ran);
                std::thread::spawn(move || {
                    let run = |grants: Vec<Grant<Item>>| {
                        let mut pending = grants;
                        while let Some(grant) = pending.pop() {
                            let units = grant.item.units;
                            let now = held.fetch_add(units, Ordering::SeqCst) + units;
                            assert!(now <= 3, "held {now} of 3 units");
                            held.fetch_sub(units, Ordering::SeqCst);
                            ran.fetch_add(1, Ordering::SeqCst);
                            pending.extend(dispatcher.complete(grant.permit, 0).started);
                        }
                    };
                    for round in 0..per_thread {
                        let units = 1 + (thread + round) % 3;
                        let admission = dispatcher
                            .submit(item("x", Priority::Normal, units), 0)
                            .expect("unbounded queue accepts");
                        run(admission.dispatched.started);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(ran.load(Ordering::SeqCst), THREADS * per_thread);
        assert_eq!(dispatcher.parked(), 0);
        assert_eq!(dispatcher.units_in_use(), 0);
    }
}
