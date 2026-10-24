//! The priority dispatcher: one owner for the parked queue and the
//! unit-weighted semaphore.
//!
//! Admission is by every capacity dimension at once (cr-23): an item's
//! [`Ticket::demand`] is a [`UnitVector`] (total units plus units per
//! resource kind), and the semaphore admits it only when it fits the total,
//! the permit bound and every kind the pool limits. "Fits" below always means
//! fits in every dimension.
//!
//! ## Protocol
//!
//! Every decision about parked work is made under one mutex (the *dispatch
//! lock*), which guards the parked queue and the sequence counter:
//!
//! - **submit**: stamp the sequence, then either start the item (queue empty
//!   and it fits), park it and run the drain (queue below depth), or, with
//!   `max_queue_depth` items parked, start it only if it would start at once
//!   anyway and reject it otherwise. The depth check and the enqueue are one
//!   critical section, so the depth limit holds exactly (no TOCTOU) and a
//!   park cannot interleave with a drain.
//! - **submit at depth**: an item that must wait is rejected with
//!   `QueueFull`, but one that the drain after a park would have started at
//!   once is started without being parked: one that precedes the head (a
//!   strictly higher priority) and fits, or one that may skip the blocked
//!   head under the starvation guard (below) and is the first parked-or-new
//!   item, in dispatch order, that fits. Both are the drain's own choices, so
//!   priority order and the guard are unchanged; the skip is counted against
//!   the head like any other. The check is conservative: when the head is
//!   expired or would fit itself (a drain is pending), the item is rejected.
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
//! ## Returning permits
//!
//! A [`Grant`]'s permit must come back through
//! [`complete`](Dispatcher::complete), which drains. A permit dropped any
//! other way (a dropped [`Grant`], [`Dispatched`] or [`Admission`], a task
//! future a spawner discarded) returns its units but cannot drain itself: it
//! calls the *release hook* instead, which the owner installs with
//! [`install_release_hook`](Dispatcher::install_release_hook) and which must
//! arrange a [`poke`](Dispatcher::poke) (the tokio pools spawn one). Without a
//! hook, the owner must poke after such a drop, or the parked work the freed
//! units admit waits for the next submission or completion. The hook is not
//! called while the thread unwinds; an owner that catches a panic pokes
//! afterwards (every pool does).
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
//! free capacity in every dimension (and a free permit): among the items that
//! *can* start, priority and FIFO order still hold. Each such start is a
//! *skip* of the head. With several dimensions, the skipped-in item may be
//! one that needs nothing of the dimension the head is short in; it is a skip
//! all the same.
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
//! of [`submit`](Dispatcher::submit), in its at-depth path or in the drain.
//! The fast path starts an item only when nothing is parked, and `H` is
//! parked. The at-depth path starts the new item either because it precedes
//! the head (it then *is* the head the drain would serve, and `H` is no
//! longer the head) or as a skip; in the drain, an item other than the head
//! starts only as a skip. A skip, in either place, requires the head's skip
//! count to be below `N` and increments it. The count is keyed by `H`'s
//! sequence and restarts only when the head's sequence differs, which would
//! contradict `H` staying the head. So at most `N` items start in place of
//! `H`.
//!
//! (2) While `H` is the head, every item holding a permit is in `R` or `S`
//! (by (1), nothing else starts). `H`'s demand is within the pool's limit in
//! *every* dimension (the total `max_units` and each limited kind; an item
//! over any limit is rejected at submission, cr-23), and a pool has at least
//! one permit, so `H` fits as soon as no permit is held: every dimension is
//! then back to zero. Every completion returns its permit under the dispatch lock and then
//! drains ([`complete`](Dispatcher::complete); a permit dropped any other way
//! calls the release hook, which pokes, or is followed by a
//! [`poke`](Dispatcher::poke) in the pools), and that drain
//! reads `H` as the head and starts it once it fits; so `H` starts no later
//! than the drain after the last completion in `R` and `S`. For the
//! wall-clock form: `H` does not fit (in any dimension) only while some
//! permit is held, so from
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
//! that starves. Under `--cfg parking_lot_negative_control="s-20-depth"` only
//! the at-depth path forgets to charge its skips to the head, and under
//! `"s-20-order"` it starts a submission ahead of a higher-priority item that
//! fits; the `loom_at_depth` model must fail under either.
//!
//! ## Capacity invariant
//!
//! Units are claimed only through [`UnitSemaphore::try_acquire_vector`],
//! which never moves the total past `max_units` nor a limited kind past its
//! limit, and claims every dimension of a demand or none (see
//! `crate::semaphore`). A started item holds a [`Permit`] until it completes,
//! so in every dimension the units of all running items never exceed the
//! limit. The `loom_capacity` and `loom_vector` models check this.
//!
//! ## No lost wakeup
//!
//! Claim: when `complete` returns, no parked head that fits the free capacity
//! is left parked. (A permit released outside `complete` gives the same
//! guarantee only through the drain its release hook or owner runs; see
//! "Returning permits".) A parked item becomes startable only when units are
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

use crate::contracts::{ResourceKind, SchedulerError};
use crate::semaphore::sync::{Arc, Mutex};
use crate::semaphore::{Permit, UnitSemaphore, UnitVector};

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
/// `benches/utilization.rs` (16 units, about 95% offered load) 8 skips
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
///
/// Return the permit with [`Dispatcher::complete`] when the item finishes.
/// Dropping the grant (or its permit) frees the units without draining: the
/// dispatcher's release hook runs instead, or, without one, the owner must
/// call [`Dispatcher::poke`]. See the module docs, "Returning permits".
#[derive(Debug)]
pub struct Grant<T> {
    /// The item to start.
    pub item: T,
    /// Its claim on the semaphore. Return it with
    /// [`Dispatcher::complete`] when the item finishes.
    pub permit: Permit,
}

/// What a dispatcher operation took off the parked queue.
///
/// Dropping it drops its grants' permits: see [`Grant`] for what that
/// obliges.
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
///
/// Dropping it drops the grants in `dispatched`: see [`Grant`] for what that
/// obliges.
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
    ///
    /// Per-dimension limits (cr-23) are the semaphore's; see
    /// [`UnitSemaphore::with_resource_limit`].
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

    /// Limit dimension `kind` to `limit` units (cr-23) on a dispatcher that
    /// has not taken a submission yet. Pools call it while they are being
    /// built.
    ///
    /// Before the first submission no permit is outstanding (every permit
    /// shares the semaphore, so exclusive access to it is checked too) and
    /// nothing is parked: a parked item over a new limit could never start,
    /// and at the head it would block the items behind it.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::InvalidConfig`] once an item has been submitted (or
    /// a permit is outstanding); nothing changes then.
    pub fn set_resource_limit(
        &mut self,
        kind: ResourceKind,
        limit: u32,
    ) -> Result<(), SchedulerError> {
        let submitted = self.state.lock().last_sequence > 0;
        let semaphore = Arc::get_mut(&mut self.semaphore).filter(|_| !submitted);
        let Some(semaphore) = semaphore else {
            return Err(SchedulerError::invalid_config(format!(
                "pool '{}': resource limits can only be set before the first submission",
                self.pool
            )));
        };
        semaphore.set_resource_limit(kind, limit);
        Ok(())
    }

    /// The pool name used in errors and logs.
    #[must_use]
    pub fn pool_name(&self) -> &str {
        &self.pool
    }

    /// Rename the pool (errors and logs). Pools call it while they are being
    /// built, before anything is submitted.
    pub fn set_pool_name(&mut self, pool: impl Into<String>) {
        self.pool = pool.into();
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

    /// The limit of dimension `kind`, if the pool limits it (cr-23).
    #[must_use]
    pub fn resource_limit(&self, kind: ResourceKind) -> Option<u32> {
        self.semaphore.resource_limit(kind)
    }

    /// Units held by started, not yet completed items.
    #[must_use]
    pub fn units_in_use(&self) -> u32 {
        self.semaphore.units_in_use()
    }

    /// Units held in dimension `kind` by started, not yet completed items, or
    /// `None` when the pool does not limit `kind` (cr-23).
    #[must_use]
    pub fn units_in_use_of(&self, kind: ResourceKind) -> Option<u32> {
        self.semaphore.units_in_use_of(kind)
    }

    /// Reject a demand that exceeds the pool's limit in some dimension (the
    /// total or a limited kind): it could never start, and parked at the head
    /// it would block the items behind it.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::CapacityExceeded`] naming the first dimension
    /// exceeded (the total first, then kinds in `ResourceKind` order: `kind`
    /// is `None` for the total), the units requested there and its limit.
    pub fn check_capacity(&self, demand: &UnitVector) -> Result<(), SchedulerError> {
        match self.semaphore.exceeded_limit(demand) {
            None => Ok(()),
            Some((kind, requested, available)) => Err(SchedulerError::capacity_exceeded(
                self.pool.clone(),
                kind,
                requested,
                available,
            )),
        }
    }

    /// Started, not yet completed items.
    #[must_use]
    pub fn running(&self) -> u32 {
        self.semaphore.permits_outstanding()
    }

    /// Install the *release hook* that `make` builds, unless one is
    /// installed (`make` is then not called). Returns whether this call
    /// installed it.
    ///
    /// The hook runs after a permit of this dispatcher is dropped other than
    /// through [`complete`](Self::complete): a dropped [`Grant`],
    /// [`Dispatched`] or [`Admission`], or a task whose future was discarded.
    /// Its job is to make a [`poke`](Self::poke) happen, so the parked work
    /// the freed units admit is started (see the module docs, "Returning
    /// permits").
    ///
    /// It runs on the dropping thread, outside the dispatch lock, possibly
    /// while that thread holds the owner's own locks: it must not block, and
    /// should *schedule* the poke (spawn it) rather than poke inline. It is
    /// not called while the thread unwinds, and a panic inside it is
    /// contained and logged.
    pub fn install_release_hook<F>(&self, make: impl FnOnce() -> F) -> bool
    where
        F: Fn() + Send + Sync + 'static,
    {
        self.semaphore.install_release_hook(make)
    }

    /// Whether a release hook is installed.
    #[must_use]
    pub fn has_release_hook(&self) -> bool {
        self.semaphore.has_release_hook()
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
    ///   than the pool's limit in some dimension: the total, or a limited
    ///   kind (it could never start; see
    ///   [`check_capacity`](Self::check_capacity));
    /// - [`SchedulerError::QueueFull`] when it must wait and `max_queue_depth`
    ///   items are already parked. An item that would start at once is
    ///   admitted even then (see the module docs, "submit at depth");
    /// - [`SchedulerError::InvalidConfig`] when the semaphore admits no
    ///   permit at all (`max_permits == 0`): nothing could ever start;
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
        let demand = item.ticket().demand;
        if self.semaphore.max_permits() == 0 {
            return Err(SchedulerError::invalid_config(format!(
                "pool '{}' admits no task: its semaphore allows 0 permits",
                self.pool
            )));
        }
        self.check_capacity(&demand)?;

        let mut state = self.state.lock();
        state.last_sequence += 1;
        let sequence = state.last_sequence;
        item.set_sequence(sequence);

        if state.queue.is_empty()
            && let Some(permit) = UnitSemaphore::try_acquire_vector(&self.semaphore, &demand)
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
            let Some(dispatched) = self.start_at_depth(&mut state, item, now_ms) else {
                return Err(SchedulerError::queue_full(
                    self.pool.clone(),
                    depth,
                    self.max_queue_depth,
                ));
            };
            drop(state);
            return Ok(Admission {
                sequence,
                started: true,
                dispatched,
            });
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
        // Quietly: this drain is the wakeup, so the release hook is not
        // needed (and a hook that poked inline would deadlock here).
        permit.release_quietly();
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
                // A queue that reports a head it then cannot pop would make
                // `continue` spin; stop as the start path does.
                match state.queue.pop_head() {
                    Some(item) => dispatched.expired.push(item),
                    None => break,
                }
                continue;
            }
            if let Some(permit) = UnitSemaphore::try_acquire_vector(&self.semaphore, &ticket.demand)
            {
                let Some(item) = state.queue.pop_head() else {
                    permit.release_quietly();
                    break;
                };
                dispatched.started.push(Grant { item, permit });
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
        let mut blocked = Self::blocked_head(state, head);
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
        let Some(free) = self.semaphore.available() else {
            return false;
        };
        let candidate = match state.queue.fitting(&free, now_ms) {
            Ok(Some(candidate)) => candidate,
            Ok(None) => return false,
            Err(error) => {
                // Nothing was removed; the next submit/complete retries.
                tracing::error!(pool = %self.pool, %error, "parked queue unreadable for skip-ahead");
                return false;
            }
        };
        let Some(permit) = UnitSemaphore::try_acquire_vector(&self.semaphore, &candidate.demand)
        else {
            return false;
        };
        let Some(item) = state.queue.take(&candidate) else {
            // Returned under the dispatch lock; this drain is still running.
            permit.release_quietly();
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

    /// The skip record of `head`: the current one if it is for `head`,
    /// otherwise a fresh one (a different item became the head).
    const fn blocked_head(state: &State<Q>, head: &Ticket) -> BlockedHead {
        match state.blocked {
            Some(blocked) if blocked.sequence == head.sequence => blocked,
            _ => BlockedHead {
                sequence: head.sequence,
                skips: 0,
                reserved: false,
            },
        }
    }

    /// Submission at depth (`max_queue_depth` items parked): start `item`
    /// now, without parking it, if the drain after a park would have started
    /// it at once; otherwise drop it and return `None` (the caller reports
    /// `QueueFull`). Callers hold the dispatch lock.
    ///
    /// The drain would start the new item at once in exactly two cases, and
    /// this checks the same conditions the drain does:
    ///
    /// - it precedes the head (strictly higher priority, as it is the newest)
    ///   and fits: it would be the head, and a head that fits starts;
    /// - it follows a head that does not fit, skip-ahead is on and the
    ///   queue supports it, the starvation guard has not engaged for that
    ///   head, the item fits, and no parked item ahead of it fits: it would
    ///   be the skip candidate. The start is counted as a skip of the head.
    ///
    /// It is conservative where the drain would first change the queue: an
    /// expired head, or a head that fits (a drain is pending), gets `None`.
    /// After a start, the drain runs as it would have after a park, and its
    /// grants are returned with the item's.
    fn start_at_depth(&self, state: &mut State<Q>, item: T, now_ms: u128) -> Option<Dispatched<T>> {
        let ticket = item.ticket();
        if ticket.is_expired(now_ms) {
            return None;
        }
        let head = state.queue.head().ok().flatten()?;
        if head.is_expired(now_ms) {
            return None;
        }
        let permit = if ticket.runs_before(&head) {
            UnitSemaphore::try_acquire_vector(&self.semaphore, &ticket.demand)?
        } else {
            self.claim_skip_at_depth(state, &ticket, &head, now_ms)?
        };
        let mut dispatched = Dispatched {
            started: vec![Grant { item, permit }],
            expired: Vec::new(),
        };
        let after = self.drain_locked(state, now_ms);
        dispatched.started.extend(after.started);
        dispatched.expired.extend(after.expired);
        Some(dispatched)
    }

    /// The skip branch of [`start_at_depth`](Self::start_at_depth): claim
    /// `ticket`'s demand as a skip of `head` if the drain would, and count
    /// the skip. Callers hold the dispatch lock.
    fn claim_skip_at_depth(
        &self,
        state: &mut State<Q>,
        ticket: &Ticket,
        head: &Ticket,
        now_ms: u128,
    ) -> Option<Permit> {
        if state.max_head_skips == 0 || !state.queue.supports_skip_ahead() {
            return None;
        }
        let blocked = Self::blocked_head(state, head);
        if guard_engaged(blocked.skips, state.max_head_skips) {
            return None;
        }
        let free = self.semaphore.available()?;
        if !ticket.can_skip_ahead(&free, now_ms)
            || !Self::first_to_fit_at_depth(state, ticket, head, &free, now_ms)
        {
            return None;
        }
        let permit = UnitSemaphore::try_acquire_vector(&self.semaphore, &ticket.demand)?;
        charge_skip_at_depth(&mut state.blocked, blocked);
        state.stats.skipped += 1;
        Some(permit)
    }
}

impl<T: Parkable, Q: ParkedQueue<T>> Dispatcher<T, Q> {
    /// Whether `ticket`, which fits `free`, is the item the drain would start
    /// first: the head does not fit, and no parked item ahead of `ticket`
    /// fits (it would be the drain's candidate instead). At
    /// `max_queue_depth = 1` the head is the only parked item, so the two
    /// checks cover the same case. Callers hold the dispatch lock.
    #[cfg(not(parking_lot_negative_control = "s-20-order"))]
    fn first_to_fit_at_depth(
        state: &mut State<Q>,
        ticket: &Ticket,
        head: &Ticket,
        free: &UnitVector,
        now_ms: u128,
    ) -> bool {
        if head.demand.fits_in(free) {
            return false;
        }
        match state.queue.fitting(free, now_ms) {
            Ok(None) => true,
            Ok(Some(parked)) => ticket.runs_before(&parked),
            Err(_) => false,
        }
    }

    /// Negative control S-20-order: neither check runs, so a submission at
    /// `max_queue_depth` starts ahead of a higher-priority head (or parked
    /// item) that fits. The `loom_at_depth` model must fail with its
    /// priority-order message.
    #[cfg(parking_lot_negative_control = "s-20-order")]
    fn first_to_fit_at_depth(
        state: &mut State<Q>,
        ticket: &Ticket,
        head: &Ticket,
        free: &UnitVector,
        now_ms: u128,
    ) -> bool {
        let _ = (state, ticket, head, free, now_ms);
        true
    }
}

/// Charge one at-depth skip to the head's record `blocked` and store it.
#[cfg(not(parking_lot_negative_control = "s-20-depth"))]
const fn charge_skip_at_depth(record: &mut Option<BlockedHead>, mut blocked: BlockedHead) {
    blocked.skips = blocked.skips.saturating_add(1);
    *record = Some(blocked);
}

/// Negative control S-20-depth: the at-depth skip is counted in the stats
/// but not charged to the head (the copy is never stored back), so
/// submissions at `max_queue_depth` can skip a blocked head without bound
/// while the drain path keeps its guard. The `loom_at_depth` model must fail.
#[cfg(parking_lot_negative_control = "s-20-depth")]
const fn charge_skip_at_depth(record: &mut Option<BlockedHead>, blocked: BlockedHead) {
    let _ = (record, blocked);
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
        /// A vector demand (cr-23); `None` is `UnitVector::scalar(units)`.
        vector: Option<UnitVector>,
        deadline_ms: Option<u128>,
        sequence: u64,
    }

    const fn item(name: &'static str, priority: Priority, units: u32) -> Item {
        Item {
            name,
            priority,
            units,
            vector: None,
            deadline_ms: None,
            sequence: 0,
        }
    }

    /// An item needing `vram` GPU VRAM units and `slots` CPU slots.
    fn vitem(name: &'static str, priority: Priority, vram: u32, slots: u32) -> Item {
        let cost = crate::contracts::ResourceCost::new(ResourceKind::GpuVram, vram)
            .with(ResourceKind::Cpu, slots);
        let demand = UnitVector::from_cost(&cost);
        Item {
            vector: Some(demand),
            ..item(name, priority, demand.total())
        }
    }

    /// A pool of 16 VRAM units and 2 slots; the total is not binding.
    fn vector_dispatcher() -> Dispatcher<Item> {
        Dispatcher::in_memory(
            "test",
            UnitSemaphore::new(u32::MAX)
                .with_resource_limit(ResourceKind::GpuVram, 16)
                .with_resource_limit(ResourceKind::Cpu, 2),
            16,
        )
    }

    impl Parkable for Item {
        fn ticket(&self) -> Ticket {
            Ticket {
                priority: self.priority,
                sequence: self.sequence,
                demand: self
                    .vector
                    .unwrap_or_else(|| UnitVector::scalar(self.units)),
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

    /// The spec scenario through the dispatcher: (8 GiB, 1 slot) waits while
    /// 8 GiB are free but no slot is, and starts when a slot is returned.
    #[test]
    fn vector_item_short_in_one_dimension_waits_until_it_fits() {
        let dispatcher = vector_dispatcher();
        let a = dispatcher
            .submit(vitem("a", Priority::Normal, 4, 1), 0)
            .expect("ok");
        let b = dispatcher
            .submit(vitem("b", Priority::Normal, 4, 1), 0)
            .expect("ok");
        assert!(a.started && b.started);
        assert_eq!(dispatcher.units_in_use_of(ResourceKind::GpuVram), Some(8));
        assert_eq!(dispatcher.units_in_use_of(ResourceKind::Cpu), Some(2));
        let c = dispatcher
            .submit(vitem("c", Priority::Normal, 8, 1), 0)
            .expect("ok");
        assert!(!c.started, "8 GiB free, 0 slots free: the task waits");
        assert_eq!(dispatcher.parked(), 1);
        let mut running = a.dispatched.started;
        let woken = dispatcher.complete(running.pop().expect("a").permit, 0);
        assert_eq!(names(&woken.started), vec!["c"]);
        assert_eq!(dispatcher.units_in_use_of(ResourceKind::GpuVram), Some(12));
        assert_eq!(dispatcher.units_in_use_of(ResourceKind::Cpu), Some(2));
        drop((b, woken));
    }

    /// Skip-ahead by vector: the head is short in VRAM; an item that needs
    /// only a slot skips ahead, one short in slots is passed over.
    #[test]
    fn starvation_vector_skip_ahead_uses_the_fit_in_every_dimension() {
        let dispatcher = vector_dispatcher();
        let blocker = dispatcher
            .submit(vitem("blocker", Priority::Normal, 10, 1), 0)
            .expect("ok");
        let head = dispatcher
            .submit(vitem("head", Priority::High, 8, 0), 0)
            .expect("ok");
        assert!(!head.started, "6 VRAM free, the head needs 8");
        let two_slots = dispatcher
            .submit(vitem("two-slots", Priority::Normal, 1, 2), 0)
            .expect("ok");
        assert!(!two_slots.started, "one slot free, it needs two");
        let one_slot = dispatcher
            .submit(vitem("one-slot", Priority::Low, 6, 1), 0)
            .expect("ok");
        assert!(one_slot.started, "fits the free VRAM and the free slot");
        assert_eq!(dispatcher.skip_ahead_stats().skipped, 1);
        assert_eq!(dispatcher.units_in_use_of(ResourceKind::GpuVram), Some(16));
        let mut woken = Vec::new();
        for grant in blocker
            .dispatched
            .started
            .into_iter()
            .chain(one_slot.dispatched.started)
        {
            woken.extend(dispatcher.complete(grant.permit, 0).started);
        }
        assert_eq!(names(&woken), vec!["head", "two-slots"]);
    }

    #[test]
    fn vector_item_over_a_kind_limit_is_rejected_at_submission() {
        let dispatcher = vector_dispatcher();
        assert!(matches!(
            dispatcher.submit(vitem("wide", Priority::Normal, 4, 3), 0),
            Err(SchedulerError::CapacityExceeded {
                kind: Some(ResourceKind::Cpu),
                requested: 3,
                available: 2,
                ..
            })
        ));
        assert!(matches!(
            dispatcher.submit(vitem("big", Priority::Normal, 17, 1), 0),
            Err(SchedulerError::CapacityExceeded {
                kind: Some(ResourceKind::GpuVram),
                requested: 17,
                available: 16,
                ..
            })
        ));
        assert_eq!(dispatcher.parked(), 0);
        assert_eq!(dispatcher.units_in_use(), 0);
    }

    #[test]
    fn resource_limits_are_set_only_before_the_first_submission() {
        let mut dispatcher = Dispatcher::in_memory("test", UnitSemaphore::new(4), 8);
        dispatcher
            .set_resource_limit(ResourceKind::GpuVram, 2)
            .expect("idle");
        assert_eq!(dispatcher.resource_limit(ResourceKind::GpuVram), Some(2));
        let running = dispatcher
            .submit(vitem("a", Priority::Normal, 2, 0), 0)
            .expect("ok");
        assert!(running.started);
        assert!(matches!(
            dispatcher.set_resource_limit(ResourceKind::Cpu, 1),
            Err(SchedulerError::InvalidConfig { .. })
        ));
        assert_eq!(dispatcher.resource_limit(ResourceKind::Cpu), None);
        drop(running);
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
