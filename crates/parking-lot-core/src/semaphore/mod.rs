//! Unit-weighted capacity semaphore shared by every pool.
//!
//! A [`UnitSemaphore`] admits work by weight: a task costing `u` units is
//! admitted only while `in_use + u <= max_units`. It can also bound the number
//! of admitted tasks (the native `WorkerPool` bounds it by its worker count so
//! that an admitted task is a running task), and, with cr-23, the units held
//! in each [`ResourceKind`] (for example `GpuVram` and `Cpu` slots): a task's
//! [`UnitVector`] demand is admitted only if it fits in *every* dimension at
//! once.
//!
//! Admission yields a [`Permit`]. Dropping the permit returns its units (RAII),
//! so capacity cannot leak even when the holder unwinds. Pools return permits
//! through [`Dispatcher::complete`](crate::dispatch::Dispatcher::complete),
//! which also starts the parked work the freed capacity admits. A permit
//! dropped any other way frees capacity and then calls the semaphore's
//! *release hook*, if one is installed
//! ([`Dispatcher::install_release_hook`](crate::dispatch::Dispatcher::install_release_hook)),
//! so that its owner can drain; without a hook it wakes nothing until the
//! next submission, completion or poke.
//!
//! ## Representation
//!
//! - **Total dimension and permits**: one packed `AtomicU64` (low half: units
//!   in use; high half: permits outstanding). Claimed with one
//!   compare-and-swap, so both are claimed together or not at all. A
//!   semaphore without kind limits is exactly this, and so is the
//!   single-dimension path: a pool configured with `max_units` alone pays one
//!   branch on an `Option` over the cr-19 semaphore.
//! - **Kind dimensions**: only for kinds given a limit
//!   ([`with_resource_limit`](UnitSemaphore::with_resource_limit)), a small
//!   array of units in use per kind behind a mutex (the *kind lock*).
//!
//! A demand that needs units of a limited kind is claimed inside one kind-lock
//! critical section: check every limited kind, then CAS the packed total and
//! permit, then add the kind units, then unlock. Its release subtracts the
//! kind units and the packed total inside one kind-lock critical section too.
//! A demand that needs no limited kind never takes the kind lock.
//!
//! ## Capacity invariant
//!
//! *Total and permits.* Every change to the packed state is a read-modify-write
//! on one atomic, and all RMWs on one location are totally ordered (its
//! modification order). An acquire writes `old + u` only after checking
//! `old + u <= max`, and a release subtracts exactly what an earlier acquire
//! added, so every value in the modification order is within both limits. This
//! holds for any memory ordering; the orderings below exist to publish the
//! *resource* between holders, not to protect the count.
//!
//! *Each limited kind.* Its counter is read and written only inside kind-lock
//! critical sections, which the lock totally orders. A claim adds `d` only
//! after checking `in_use + d <= limit` in the same critical section, and a
//! release subtracts exactly what its claim added, so every value the counter
//! takes is within its limit.
//!
//! *Atomic across dimensions (no partial claim is visible to another
//! claimer).* A claimer that needs a limited kind reads the kind counters and
//! CASes the packed state inside its kind-lock critical section; any other
//! claim or release that touches a kind counter is a whole critical section
//! before or after it, so it sees every dimension of that claim or none. A
//! claimer that needs no limited kind reads only the packed state, where
//! another claim's total and permit appear in one CAS. A claim that fails
//! changes nothing: the kind check precedes the CAS, and the kind units are
//! added only after the CAS succeeded.
//!
//! The admission checks are the scheduler's capacity guarantee. Under the
//! test-only `--cfg parking_lot_negative_control="s-1"` the packed check is
//! skipped (units are still counted), and the `loom_capacity` model must then
//! report a violation. Under `--cfg parking_lot_negative_control="s-19"` only
//! the first dimension (the packed total and permits) is checked and the kind
//! limits are not, and the `loom_vector` model must then report a violation.

pub(crate) mod sync;
mod vector;

pub use self::vector::{KIND_COUNT, UnitVector};

use std::sync::OnceLock;

use self::sync::{Arc, AtomicU64, Mutex, Ordering};
use self::vector::{KINDS, kind_index};
use crate::contracts::ResourceKind;

/// One permit in the high half of the packed state.
const ONE_PERMIT: u64 = 1 << 32;
/// Mask of the units in the low half of the packed state.
const UNITS_MASK: u64 = (1 << 32) - 1;

/// Units in use in a packed state value (the low half, a `u32` by
/// construction).
const fn units_of(state: u64) -> u32 {
    (state & UNITS_MASK) as u32
}

/// Permits outstanding in a packed state value (the high half).
const fn permits_of(state: u64) -> u32 {
    (state >> 32) as u32
}

/// The packed-state delta of one claim of `units` units.
fn packed_delta(units: u32) -> u64 {
    ONE_PERMIT | u64::from(units)
}

/// Per-kind limits and the units held per limited kind.
#[derive(Debug)]
struct KindLimits {
    /// Limit per kind (`kind_index` order); `None` leaves the kind bounded
    /// only by the total.
    limits: [Option<u32>; KIND_COUNT],
    /// Units held per kind. Read and written only under this lock (the kind
    /// lock); slots of kinds without a limit stay zero.
    in_use: Mutex<[u32; KIND_COUNT]>,
}

impl KindLimits {
    fn new() -> Self {
        Self {
            limits: [None; KIND_COUNT],
            in_use: Mutex::new([0; KIND_COUNT]),
        }
    }

    /// Whether `demand` needs units of a limited kind (and so takes the kind
    /// lock). Fixed for a given demand: limits do not change while permits
    /// are outstanding.
    fn touches(&self, demand: &UnitVector) -> bool {
        self.limits
            .iter()
            .zip(demand.kinds())
            .any(|(limit, &units)| limit.is_some() && units > 0)
    }

    /// Whether `demand` fits every limited kind given `in_use`.
    #[cfg(not(parking_lot_negative_control = "s-19"))]
    fn fits(&self, in_use: &[u32; KIND_COUNT], demand: &UnitVector) -> bool {
        self.limits
            .iter()
            .zip(in_use)
            .zip(demand.kinds())
            .all(|((limit, &held), &units)| {
                limit.is_none_or(|limit| held.checked_add(units).is_some_and(|sum| sum <= limit))
            })
    }

    /// Negative control S-19: only the first dimension (the packed total and
    /// permits) is checked; the kind limits are not. Kind units are still
    /// counted, so release stays balanced.
    #[cfg(parking_lot_negative_control = "s-19")]
    fn fits(&self, in_use: &[u32; KIND_COUNT], demand: &UnitVector) -> bool {
        let _ = self.limits;
        in_use
            .iter()
            .zip(demand.kinds())
            .all(|(&held, &units)| held.checked_add(units).is_some())
    }

    /// Add (`claim`) or subtract `demand`'s units of every limited kind.
    /// Callers hold the kind lock; a claim was checked by `fits`, a release
    /// subtracts what its claim added, so neither overflows.
    fn apply(&self, in_use: &mut [u32; KIND_COUNT], demand: &UnitVector, claim: bool) {
        for ((held, limit), &units) in in_use.iter_mut().zip(&self.limits).zip(demand.kinds()) {
            if limit.is_some() {
                if claim {
                    *held += units;
                } else {
                    debug_assert!(*held >= units, "kind released more than was acquired");
                    *held -= units;
                }
            }
        }
    }

    /// Free units per kind given `in_use`; `u32::MAX` for a kind without a
    /// limit.
    fn free(&self, in_use: &[u32; KIND_COUNT]) -> [u32; KIND_COUNT] {
        let mut free = [u32::MAX; KIND_COUNT];
        for ((slot, limit), &held) in free.iter_mut().zip(&self.limits).zip(in_use) {
            if let Some(limit) = limit {
                *slot = limit.saturating_sub(held);
            }
        }
        free
    }
}

/// Called after a [`Permit`] is dropped other than through
/// [`Dispatcher::complete`](crate::dispatch::Dispatcher::complete).
struct ReleaseHook(Box<dyn Fn() + Send + Sync>);

impl std::fmt::Debug for ReleaseHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReleaseHook")
    }
}

/// A unit-weighted counting semaphore with an optional bound on permits and
/// optional per-kind limits (cr-23).
#[derive(Debug)]
pub struct UnitSemaphore {
    max_units: u32,
    max_permits: u32,
    /// Low 32 bits: units in use. High 32 bits: permits outstanding.
    state: AtomicU64,
    /// `None` until a kind gets a limit: then the single-dimension path never
    /// takes a lock.
    kinds: Option<KindLimits>,
    /// Set at most once, by the dispatcher's owner. Read (one `Acquire`
    /// load, `OnceLock::get`) only when a permit is dropped outside
    /// `Dispatcher::complete`; that load synchronizes-with the `Release` in
    /// `OnceLock::set`, so the hook's captured state is visible to the
    /// dropping thread.
    release_hook: OnceLock<ReleaseHook>,
}

impl UnitSemaphore {
    /// A semaphore of `max_units` units and no bound on the number of permits.
    #[must_use]
    pub fn new(max_units: u32) -> Self {
        Self::with_max_permits(max_units, u32::MAX)
    }

    /// A semaphore of `max_units` units that also admits at most
    /// `max_permits` holders at once.
    ///
    /// `max_permits == 0` admits nothing; a
    /// [`Dispatcher`](crate::dispatch::Dispatcher) over such a semaphore
    /// refuses every submission with `SchedulerError::InvalidConfig` rather
    /// than parking work that could never start.
    #[must_use]
    #[cfg_attr(
        not(parking_lot_loom),
        expect(
            clippy::missing_const_for_fn,
            reason = "loom's AtomicU64::new is not const; one signature for both builds"
        )
    )]
    pub fn with_max_permits(max_units: u32, max_permits: u32) -> Self {
        Self {
            max_units,
            max_permits,
            state: AtomicU64::new(0),
            kinds: None,
            release_hook: OnceLock::new(),
        }
    }

    /// Also limit the units held in dimension `kind` to `limit` (cr-23). A
    /// demand is then admitted only while it fits the total, the permit bound
    /// and every limited kind at once. A kind without a limit is bounded only
    /// by the total. Setting a kind again replaces its limit.
    #[must_use]
    pub fn with_resource_limit(mut self, kind: ResourceKind, limit: u32) -> Self {
        self.set_resource_limit(kind, limit);
        self
    }

    /// [`with_resource_limit`](Self::with_resource_limit) for every
    /// `(kind, limit)` pair.
    #[must_use]
    pub fn with_resource_limits(
        self,
        limits: impl IntoIterator<Item = (ResourceKind, u32)>,
    ) -> Self {
        limits.into_iter().fold(self, |semaphore, (kind, limit)| {
            semaphore.with_resource_limit(kind, limit)
        })
    }

    /// Set the limit of dimension `kind`. Exclusive access means no permit is
    /// outstanding (a permit holds a shared reference), so no kind counter is
    /// in use.
    pub fn set_resource_limit(&mut self, kind: ResourceKind, limit: u32) {
        self.kinds.get_or_insert_with(KindLimits::new).limits[kind_index(kind)] = Some(limit);
    }

    /// Total units.
    #[must_use]
    pub const fn max_units(&self) -> u32 {
        self.max_units
    }

    /// Bound on outstanding permits (`u32::MAX` when unbounded).
    #[must_use]
    pub const fn max_permits(&self) -> u32 {
        self.max_permits
    }

    /// The limit of dimension `kind`, if it has one.
    #[must_use]
    pub fn resource_limit(&self, kind: ResourceKind) -> Option<u32> {
        self.kinds
            .as_ref()
            .and_then(|kinds| kinds.limits[kind_index(kind)])
    }

    /// Every `(kind, limit)` pair, in `ResourceKind` order.
    pub fn resource_limits(&self) -> impl Iterator<Item = (ResourceKind, u32)> + '_ {
        KINDS
            .into_iter()
            .filter_map(|kind| self.resource_limit(kind).map(|limit| (kind, limit)))
    }

    /// Units currently held by permits (the total dimension).
    ///
    /// `Acquire` pairs with the `Release` in [`Permit`]'s drop: a reader that
    /// observes a holder's units returned also observes everything that
    /// holder did before returning them.
    #[must_use]
    pub fn units_in_use(&self) -> u32 {
        units_of(self.state.load(Ordering::Acquire))
    }

    /// Units currently held in dimension `kind`, or `None` when the kind has
    /// no limit (its units are then counted only in the total).
    ///
    /// Read under the kind lock, whose unlock → lock edge orders it after
    /// every claim and release of that kind that unlocked before.
    #[must_use]
    pub fn units_in_use_of(&self, kind: ResourceKind) -> Option<u32> {
        let kinds = self.kinds.as_ref()?;
        let index = kind_index(kind);
        kinds.limits[index]?;
        Some(kinds.in_use.lock()[index])
    }

    /// Permits currently outstanding.
    ///
    /// `Acquire` for the same reason as [`units_in_use`](Self::units_in_use).
    #[must_use]
    pub fn permits_outstanding(&self) -> u32 {
        permits_of(self.state.load(Ordering::Acquire))
    }

    /// Total units free for one more holder: `Some(max_units - units_in_use)`
    /// while a permit is free, `None` when every permit is out. Kind limits
    /// are not considered; see [`available`](Self::available).
    #[must_use]
    pub fn available_units(&self) -> Option<u32> {
        self.available().map(|free| free.total())
    }

    /// Free capacity for one more holder in every dimension, or `None` when
    /// every permit is out. A kind without a limit is free without bound
    /// (`u32::MAX`).
    ///
    /// The dispatcher reads it under its dispatch lock to choose a skip-ahead
    /// candidate (cr-20); the claim itself is still [`try_acquire_vector`]'s
    /// check. For the packed state, `Relaxed` is enough: units are claimed
    /// only by `try_acquire_vector` under the dispatch lock, and every earlier
    /// claim is sequenced before an unlock that synchronizes-with this
    /// caller's lock, so by write-read coherence the load returns that claim's
    /// value or a later one. Later values only return units (a permit dropped
    /// outside the lock), so a stale read can only under-report the free
    /// units, which makes the choice conservative, never wrong: the claim
    /// re-checks the real state. The kind counters are read under the kind
    /// lock (unlock → lock edge), so they are current.
    ///
    /// [`try_acquire_vector`]: Self::try_acquire_vector
    #[must_use]
    #[inline]
    pub fn available(&self) -> Option<UnitVector> {
        let state = self.state.load(Ordering::Relaxed);
        if permits_of(state) >= self.max_permits {
            return None;
        }
        let total = self.max_units.saturating_sub(units_of(state));
        let kinds = self.kinds.as_ref().map_or([u32::MAX; KIND_COUNT], |kinds| {
            kinds.free(&kinds.in_use.lock())
        });
        Some(UnitVector::free(total, kinds))
    }

    /// The first dimension in which `demand` exceeds this semaphore's limit,
    /// as `(kind, requested, limit)` (`kind` is `None` for the total), or
    /// `None` when it fits an idle semaphore. Such a demand can never be
    /// admitted; pools reject it at submission.
    ///
    /// A demand whose per-kind units sum past `u32::MAX` exceeds the total
    /// whatever `max_units` is: [`UnitVector::from_cost`] saturates its total
    /// at `u32::MAX`, which is reported as `requested`.
    #[must_use]
    #[inline]
    pub fn exceeded_limit(&self, demand: &UnitVector) -> Option<(Option<ResourceKind>, u32, u32)> {
        if demand.total() > self.max_units || demand.total_overflows() {
            return Some((None, demand.total(), self.max_units));
        }
        // Single-dimension pools stop here: this runs on every submission.
        self.kinds.as_ref()?;
        self.resource_limits().find_map(|(kind, limit)| {
            let requested = demand.of(kind);
            (requested > limit).then_some((Some(kind), requested, limit))
        })
    }

    /// Install `make()` as the release hook unless one is installed; `make`
    /// is called at most once. Returns whether this call installed it. See
    /// [`Dispatcher::install_release_hook`](crate::dispatch::Dispatcher::install_release_hook).
    pub(crate) fn install_release_hook<F>(&self, make: impl FnOnce() -> F) -> bool
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut installed = false;
        let _ = self.release_hook.get_or_init(|| {
            installed = true;
            ReleaseHook(Box::new(make()))
        });
        installed
    }

    /// Whether a release hook is installed.
    pub(crate) fn has_release_hook(&self) -> bool {
        self.release_hook.get().is_some()
    }

    /// After a permit was dropped other than through `Dispatcher::complete`:
    /// call the release hook, if any. Not while the thread unwinds (a panic
    /// inside the hook would then abort the process; the pools poke after
    /// they catch the panic instead), and a panic in the hook is contained
    /// and logged rather than escaping the permit's `drop`.
    #[cold]
    fn after_stray_release(&self) {
        let Some(hook) = self.release_hook.get() else {
            return;
        };
        if std::thread::panicking() {
            return;
        }
        let called = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (hook.0)()));
        if called.is_err() {
            tracing::error!(
                "semaphore release hook panicked; parked work may wait for the next completion"
            );
        }
    }

    /// Claim `units` units of the total dimension and one permit if both fit;
    /// otherwise claim nothing and return `None`. The single-dimension form of
    /// [`try_acquire_vector`](Self::try_acquire_vector).
    ///
    /// Takes the `Arc` explicitly (rather than `self: &Arc<Self>`) because the
    /// loom build substitutes loom's `Arc`, which cannot be a method receiver.
    #[must_use]
    #[inline]
    pub fn try_acquire(this: &Arc<Self>, units: u32) -> Option<Permit> {
        Self::try_acquire_vector(this, &UnitVector::scalar(units))
    }

    /// Claim `demand` in every dimension (total units, one permit, and the
    /// units of every limited kind) if it fits in all of them at once;
    /// otherwise claim nothing and return `None`. See the module docs for why
    /// no other claimer can see part of the claim.
    #[must_use]
    #[inline]
    pub fn try_acquire_vector(this: &Arc<Self>, demand: &UnitVector) -> Option<Permit> {
        if let Some(kinds) = &this.kinds
            && kinds.touches(demand)
        {
            return Self::claim_with_kinds(this, kinds, demand);
        }
        Self::claim_packed(this, demand)
    }

    /// The claim of a demand that needs a limited kind: the kind check, the
    /// packed CAS and the kind units in one kind-lock critical section.
    ///
    /// Out of line so that the single-dimension path (`claim_packed` alone)
    /// does not carry the lock path's frame. Measured on
    /// `benches/utilization.rs` (strict, single-dimension): with both
    /// paths in one function the claim took about 50% more profile samples
    /// and the simulation ran about 6% slower than the cr-20 semaphore;
    /// split, about 2%.
    #[inline(never)]
    fn claim_with_kinds(
        this: &Arc<Self>,
        kinds: &KindLimits,
        demand: &UnitVector,
    ) -> Option<Permit> {
        let mut in_use = kinds.in_use.lock();
        if !kinds.fits(&in_use, demand) {
            return None;
        }
        let permit = Self::claim_packed(this, demand)?;
        kinds.apply(&mut in_use, demand, true);
        drop(in_use);
        Some(permit)
    }

    /// The packed claim: `demand.total()` units and one permit, by CAS.
    #[inline(always)]
    #[expect(
        clippy::inline_always,
        reason = "the single-dimension hot path; see claim_with_kinds for the measurement"
    )]
    fn claim_packed(this: &Arc<Self>, demand: &UnitVector) -> Option<Permit> {
        let units = demand.total();
        let delta = packed_delta(units);
        // Relaxed: this read only seeds the CAS loop; the successful CAS is
        // the synchronizing access.
        let mut current = this.state.load(Ordering::Relaxed);
        loop {
            if !this.fits(current, units) {
                return None;
            }
            // Success `Acquire`: the CAS reads a value in the release sequence
            // of every earlier `Release` decrement (later RMWs by any thread
            // extend a release sequence), so it synchronizes-with the drop of
            // each permit whose units it is reusing: that holder's work
            // (freeing VRAM, delivering its result) happens-before this holder
            // starts. Failure `Relaxed`: a failed CAS publishes nothing and
            // only retries with the fresher value.
            match this.state.compare_exchange_weak(
                current,
                current + delta,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => {
                    return Some(Permit {
                        semaphore: Arc::clone(this),
                        demand: *demand,
                        wake: true,
                    });
                }
                Err(actual) => current = actual,
            }
        }
    }

    /// Whether `units` more units and one more permit fit in `state`.
    #[cfg(not(parking_lot_negative_control = "s-1"))]
    fn fits(&self, state: u64, units: u32) -> bool {
        let units_fit = units_of(state)
            .checked_add(units)
            .is_some_and(|total| total <= self.max_units);
        units_fit && permits_of(state) < self.max_permits
    }

    /// Negative control S-1: the admission check is skipped. Units are still
    /// counted (so release stays balanced), but nothing stops the sum from
    /// passing `max_units`.
    #[cfg(parking_lot_negative_control = "s-1")]
    fn fits(&self, state: u64, units: u32) -> bool {
        let _ = (self.max_units, self.max_permits);
        // Keep the packed halves from overflowing into each other.
        units_of(state).checked_add(units).is_some()
    }

    /// Return `demand` in every dimension.
    #[inline]
    fn release(&self, demand: &UnitVector) {
        if let Some(kinds) = &self.kinds
            && kinds.touches(demand)
        {
            self.release_with_kinds(kinds, demand);
            return;
        }
        self.release_packed(demand.total());
    }

    /// Release of a demand that holds limited kinds. Kind units and the
    /// packed total leave in one kind-lock critical section, so a claimer
    /// under the kind lock sees both returned or neither. Out of line for the
    /// same reason as `claim_with_kinds`.
    #[inline(never)]
    #[cfg(not(all(parking_lot_loom, parking_lot_negative_control = "oa-11-kind-release")))]
    fn release_with_kinds(&self, kinds: &KindLimits, demand: &UnitVector) {
        let mut in_use = kinds.in_use.lock();
        kinds.apply(&mut in_use, demand, false);
        self.release_packed(demand.total());
        drop(in_use);
    }

    /// OA-11 model-only control: split the read/update/write critical section.
    /// Two releasers can read the same counters and overwrite one another's
    /// returned kind units. Packed decrements remain balanced, so the model
    /// must identify the leaked kind units rather than a packed underflow.
    #[cfg(all(parking_lot_loom, parking_lot_negative_control = "oa-11-kind-release"))]
    fn release_with_kinds(&self, kinds: &KindLimits, demand: &UnitVector) {
        let mut stale = *kinds.in_use.lock();
        kinds.apply(&mut stale, demand, false);
        self.release_packed(demand.total());
        *kinds.in_use.lock() = stale;
    }

    /// Return `units` units and one permit to the packed state.
    #[inline]
    fn release_packed(&self, units: u32) {
        let delta = packed_delta(units);
        // `Release`: everything this holder did happens-before any later
        // `Acquire` CAS or load that reads this decrement (or a later value in
        // its release sequence). See `claim_packed`.
        let previous = self.state.fetch_sub(delta, Ordering::Release);
        debug_assert!(
            units_of(previous) >= units && permits_of(previous) >= 1,
            "semaphore released more than was acquired"
        );
    }
}

/// Proof of admission: a [`UnitVector`] demand and one permit of a
/// [`UnitSemaphore`].
///
/// Dropping the permit returns them. Return it through
/// [`Dispatcher::complete`](crate::dispatch::Dispatcher::complete), which also
/// starts the parked work the freed units admit; a permit dropped any other
/// way calls the semaphore's release hook (see the module docs).
#[derive(Debug)]
#[must_use = "dropping a permit releases its units immediately"]
pub struct Permit {
    semaphore: Arc<UnitSemaphore>,
    demand: UnitVector,
    /// Whether dropping it calls the release hook: `false` once the
    /// dispatcher returns it under its own lock and drains itself.
    wake: bool,
}

impl Permit {
    /// Units this permit holds in the total dimension.
    #[must_use]
    pub const fn units(&self) -> u32 {
        self.demand.total()
    }

    /// Units this permit holds in every dimension.
    #[must_use]
    pub const fn demand(&self) -> &UnitVector {
        &self.demand
    }

    /// Return the units without calling the release hook: the dispatcher
    /// drops a permit this way under its dispatch lock, where it drains
    /// itself (and where a hook that pokes would deadlock).
    #[inline]
    pub(crate) fn release_quietly(mut self) {
        self.wake = false;
    }
}

impl Drop for Permit {
    #[inline]
    fn drop(&mut self) {
        self.semaphore.release(&self.demand);
        if self.wake {
            self.semaphore.after_stray_release();
        }
    }
}

#[cfg(all(test, not(parking_lot_loom)))]
mod tests {
    use super::*;

    #[test]
    fn admits_by_weight_and_releases_on_drop() {
        let semaphore = Arc::new(UnitSemaphore::new(5));
        let three = UnitSemaphore::try_acquire(&semaphore, 3).expect("3 of 5 fits");
        let two = UnitSemaphore::try_acquire(&semaphore, 2).expect("5 of 5 fits");
        assert!(UnitSemaphore::try_acquire(&semaphore, 1).is_none());
        assert_eq!(semaphore.units_in_use(), 5);
        assert_eq!(semaphore.permits_outstanding(), 2);
        drop(three);
        assert_eq!(semaphore.units_in_use(), 2);
        assert!(UnitSemaphore::try_acquire(&semaphore, 4).is_none());
        let three_again = UnitSemaphore::try_acquire(&semaphore, 3).expect("fits again");
        drop((two, three_again));
        assert_eq!(semaphore.units_in_use(), 0);
        assert_eq!(semaphore.permits_outstanding(), 0);
    }

    #[test]
    fn permit_bound_limits_holders_including_zero_cost() {
        let semaphore = Arc::new(UnitSemaphore::with_max_permits(10, 2));
        let a = UnitSemaphore::try_acquire(&semaphore, 0).expect("first holder");
        let b = UnitSemaphore::try_acquire(&semaphore, 1).expect("second holder");
        assert!(UnitSemaphore::try_acquire(&semaphore, 0).is_none());
        drop(a);
        let c = UnitSemaphore::try_acquire(&semaphore, 0).expect("slot freed");
        drop((b, c));
        assert_eq!(semaphore.permits_outstanding(), 0);
    }

    #[test]
    fn oversized_requests_are_refused() {
        let semaphore = Arc::new(UnitSemaphore::new(u32::MAX));
        let big = UnitSemaphore::try_acquire(&semaphore, u32::MAX).expect("exactly max");
        assert!(UnitSemaphore::try_acquire(&semaphore, 1).is_none());
        drop(big);
        let small = Arc::new(UnitSemaphore::new(3));
        assert!(UnitSemaphore::try_acquire(&small, 4).is_none());
    }

    /// Threads hammer one semaphore; an independent counter of held units
    /// must never pass the limit. This is also the miri target for the
    /// semaphore's atomic path.
    #[test]
    fn concurrent_holders_never_exceed_capacity() {
        use std::sync::atomic::AtomicU32;
        const THREADS: u32 = 4;
        let rounds: u32 = if cfg!(miri) { 20 } else { 2_000 };
        let semaphore = Arc::new(UnitSemaphore::new(5));
        let held = Arc::new(AtomicU32::new(0));
        let handles: Vec<_> = (0..THREADS)
            .map(|thread| {
                let semaphore = Arc::clone(&semaphore);
                let held = Arc::clone(&held);
                std::thread::spawn(move || {
                    for round in 0..rounds {
                        let units = 1 + (thread + round) % 3;
                        if let Some(permit) = UnitSemaphore::try_acquire(&semaphore, units) {
                            let now = held.fetch_add(units, Ordering::SeqCst) + units;
                            assert!(now <= 5, "held {now} units of 5");
                            held.fetch_sub(units, Ordering::SeqCst);
                            drop(permit);
                        }
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker thread");
        }
        assert_eq!(semaphore.units_in_use(), 0);
    }

    use crate::contracts::{ResourceCost, ResourceKind};

    fn vram_and_slots(vram: u32, slots: u32) -> UnitVector {
        UnitVector::from_cost(
            &ResourceCost::new(ResourceKind::GpuVram, vram).with(ResourceKind::Cpu, slots),
        )
    }

    /// The spec scenario: 8 GiB free and no slot free, so a task needing
    /// (8 GiB, 1 slot) waits; it is admitted once a slot is returned.
    #[test]
    fn vector_task_short_in_one_dimension_waits() {
        let semaphore = Arc::new(
            UnitSemaphore::new(u32::MAX)
                .with_resource_limit(ResourceKind::GpuVram, 16)
                .with_resource_limit(ResourceKind::Cpu, 1),
        );
        let first = UnitSemaphore::try_acquire_vector(&semaphore, &vram_and_slots(8, 1))
            .expect("fits every dimension");
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(8));
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Cpu), Some(1));
        assert!(
            UnitSemaphore::try_acquire_vector(&semaphore, &vram_and_slots(8, 1)).is_none(),
            "8 GiB are free but no slot is"
        );
        // A failed claim changed nothing.
        assert_eq!(semaphore.units_in_use(), 9);
        assert_eq!(semaphore.permits_outstanding(), 1);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(8));
        // A demand that needs no slot still fits the free VRAM.
        let vram_only = UnitSemaphore::try_acquire_vector(&semaphore, &vram_and_slots(8, 0))
            .expect("VRAM only");
        drop(first);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Cpu), Some(0));
        assert!(UnitSemaphore::try_acquire_vector(&semaphore, &vram_and_slots(8, 1)).is_some());
        drop(vram_only);
        assert_eq!(semaphore.units_in_use(), 0);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(0));
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Io), None);
    }

    #[test]
    fn vector_limits_exceeded_and_free_capacity() {
        let semaphore = Arc::new(
            UnitSemaphore::with_max_permits(20, 4)
                .with_resource_limits([(ResourceKind::GpuVram, 16), (ResourceKind::Cpu, 2)]),
        );
        assert_eq!(
            semaphore.exceeded_limit(&vram_and_slots(16, 3)),
            Some((Some(ResourceKind::Cpu), 3, 2))
        );
        assert_eq!(
            semaphore.exceeded_limit(&UnitVector::scalar(21)),
            Some((None, 21, 20))
        );
        assert_eq!(semaphore.exceeded_limit(&vram_and_slots(16, 2)), None);
        let held =
            UnitSemaphore::try_acquire_vector(&semaphore, &vram_and_slots(10, 1)).expect("fits");
        let free = semaphore.available().expect("permits left");
        assert_eq!(free.total(), 9);
        assert_eq!(free.of(ResourceKind::GpuVram), 6);
        assert_eq!(free.of(ResourceKind::Cpu), 1);
        assert_eq!(free.of(ResourceKind::Io), u32::MAX);
        assert!(vram_and_slots(6, 1).fits_in(&free));
        assert!(!vram_and_slots(7, 1).fits_in(&free));
        drop(held);
        assert_eq!(
            semaphore.resource_limits().collect::<Vec<_>>(),
            vec![(ResourceKind::Cpu, 2), (ResourceKind::GpuVram, 16)]
        );
    }

    /// Threads claim mixed demands on a two-kind semaphore; independent
    /// counters per dimension must never pass a limit. The miri target for
    /// the kind-lock path.
    #[test]
    fn vector_concurrent_holders_never_exceed_any_dimension() {
        use std::sync::atomic::AtomicU32;
        const THREADS: u32 = 4;
        let rounds: u32 = if cfg!(miri) { 20 } else { 2_000 };
        let semaphore = Arc::new(
            UnitSemaphore::with_max_permits(100, 3)
                .with_resource_limit(ResourceKind::GpuVram, 5)
                .with_resource_limit(ResourceKind::Cpu, 2),
        );
        let vram = Arc::new(AtomicU32::new(0));
        let slots = Arc::new(AtomicU32::new(0));
        let handles: Vec<_> = (0..THREADS)
            .map(|thread| {
                let semaphore = Arc::clone(&semaphore);
                let (vram, slots) = (Arc::clone(&vram), Arc::clone(&slots));
                std::thread::spawn(move || {
                    for round in 0..rounds {
                        let need = vram_and_slots(1 + (thread + round) % 3, (thread + round) % 2);
                        let Some(permit) = UnitSemaphore::try_acquire_vector(&semaphore, &need)
                        else {
                            continue;
                        };
                        let (v, c) = (need.of(ResourceKind::GpuVram), need.of(ResourceKind::Cpu));
                        let held_vram = vram.fetch_add(v, Ordering::SeqCst) + v;
                        let held_slots = slots.fetch_add(c, Ordering::SeqCst) + c;
                        assert!(held_vram <= 5, "held {held_vram} VRAM of 5");
                        assert!(held_slots <= 2, "held {held_slots} slots of 2");
                        vram.fetch_sub(v, Ordering::SeqCst);
                        slots.fetch_sub(c, Ordering::SeqCst);
                        drop(permit);
                    }
                })
            })
            .collect();
        for handle in handles {
            handle.join().expect("worker thread");
        }
        assert_eq!(semaphore.units_in_use(), 0);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(0));
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Cpu), Some(0));
    }
}
