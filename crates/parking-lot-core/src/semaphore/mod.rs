//! Unit-weighted capacity semaphore shared by every pool.
//!
//! A [`UnitSemaphore`] admits work by weight: a task costing `u` units is
//! admitted only while `in_use + u <= max_units`. It can also bound the number
//! of admitted tasks (the native `WorkerPool` bounds it by its worker count so
//! that an admitted task is a running task). Admission is a single
//! compare-and-swap on one packed `AtomicU64`, so both limits are claimed
//! together or not at all.
//!
//! Admission yields a [`Permit`]. Dropping the permit returns its units (RAII),
//! so capacity cannot leak even when the holder unwinds. Pools return permits
//! through [`Dispatcher::complete`](crate::dispatch::Dispatcher::complete),
//! which also starts the parked work the freed capacity admits; a permit
//! dropped any other way frees capacity but wakes nothing until the next
//! submission or completion (cr-21 routes unwinding through `complete`).
//!
//! ## Capacity invariant
//!
//! Every change to the packed state is a read-modify-write on one atomic, and
//! all RMWs on one location are totally ordered (its modification order). An
//! acquire writes `old + u` only after checking `old + u <= max`, and a
//! release subtracts exactly what an earlier acquire added, so every value in
//! the modification order is within both limits. This holds for any memory
//! ordering; the orderings below exist to publish the *resource* between
//! holders, not to protect the count.
//!
//! The admission check is the scheduler's capacity guarantee. Under the
//! test-only `--cfg parking_lot_negative_control="s-1"` it is skipped (units
//! are still counted), and the `loom_capacity` model must then report a
//! violation.

pub(crate) mod sync;

use self::sync::{Arc, AtomicU64, Ordering};

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

/// A unit-weighted counting semaphore with an optional bound on permits.
#[derive(Debug)]
pub struct UnitSemaphore {
    max_units: u32,
    max_permits: u32,
    /// Low 32 bits: units in use. High 32 bits: permits outstanding.
    state: AtomicU64,
}

impl UnitSemaphore {
    /// A semaphore of `max_units` units and no bound on the number of permits.
    #[must_use]
    pub fn new(max_units: u32) -> Self {
        Self::with_max_permits(max_units, u32::MAX)
    }

    /// A semaphore of `max_units` units that also admits at most
    /// `max_permits` holders at once.
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
        }
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

    /// Units currently held by permits.
    ///
    /// `Acquire` pairs with the `Release` in [`Permit`]'s drop: a reader that
    /// observes a holder's units returned also observes everything that
    /// holder did before returning them.
    #[must_use]
    pub fn units_in_use(&self) -> u32 {
        units_of(self.state.load(Ordering::Acquire))
    }

    /// Permits currently outstanding.
    ///
    /// `Acquire` for the same reason as [`units_in_use`](Self::units_in_use).
    #[must_use]
    pub fn permits_outstanding(&self) -> u32 {
        permits_of(self.state.load(Ordering::Acquire))
    }

    /// Claim `units` units and one permit if both fit; otherwise claim
    /// nothing and return `None`.
    ///
    /// Takes the `Arc` explicitly (rather than `self: &Arc<Self>`) because the
    /// loom build substitutes loom's `Arc`, which cannot be a method receiver.
    #[must_use]
    pub fn try_acquire(this: &Arc<Self>, units: u32) -> Option<Permit> {
        let delta = ONE_PERMIT | u64::from(units);
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
                        units,
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

    /// Return `units` units and one permit.
    fn release(&self, units: u32) {
        let delta = ONE_PERMIT | u64::from(units);
        // `Release`: everything this holder did happens-before any later
        // `Acquire` CAS or load that reads this decrement (or a later value in
        // its release sequence). See `try_acquire`.
        let previous = self.state.fetch_sub(delta, Ordering::Release);
        debug_assert!(
            units_of(previous) >= units && permits_of(previous) >= 1,
            "semaphore released more than was acquired"
        );
    }
}

/// Proof of admission: `units` units and one permit of a [`UnitSemaphore`].
///
/// Dropping the permit returns them.
#[derive(Debug)]
#[must_use = "dropping a permit releases its units immediately"]
pub struct Permit {
    semaphore: Arc<UnitSemaphore>,
    units: u32,
}

impl Permit {
    /// Units this permit holds.
    #[must_use]
    pub const fn units(&self) -> u32 {
        self.units
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.semaphore.release(self.units);
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
}
