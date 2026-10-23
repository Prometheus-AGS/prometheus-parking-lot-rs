//! Per-dimension unit vectors (cr-23): what an item needs, and what a
//! semaphore has free, in every capacity dimension at once.
//!
//! A [`UnitVector`] has one *total* dimension (the sum of the units over every
//! [`ResourceKind`], admitted against the pool's `max_units`) and one dimension
//! per resource kind. It is `Copy` and allocation-free, so the dispatcher can
//! carry it in every [`Ticket`](crate::dispatch::Ticket) without cost on the
//! single-dimension path.

use crate::contracts::{ResourceCost, ResourceKind};

/// Number of [`ResourceKind`] dimensions a [`UnitVector`] holds.
pub const KIND_COUNT: usize = 4;

/// Every resource kind, in [`kind_index`] order (the `ResourceKind` order).
pub(super) const KINDS: [ResourceKind; KIND_COUNT] = [
    ResourceKind::Cpu,
    ResourceKind::GpuVram,
    ResourceKind::Io,
    ResourceKind::Mixed,
];

/// The dimension index of `kind`. The match is exhaustive inside this crate,
/// so a new `ResourceKind` variant fails to compile here until it has a slot.
pub(super) const fn kind_index(kind: ResourceKind) -> usize {
    match kind {
        ResourceKind::Cpu => 0,
        ResourceKind::GpuVram => 1,
        ResourceKind::Io => 2,
        ResourceKind::Mixed => 3,
    }
}

/// Units per capacity dimension: a total, plus units per [`ResourceKind`].
///
/// As a *demand* (an item's cost) the total is the sum of the per-kind units
/// ([`from_cost`](Self::from_cost)), or a bare count with no kinds
/// ([`scalar`](Self::scalar)). As *free capacity*
/// ([`UnitSemaphore::available`](super::UnitSemaphore::available)) a kind
/// without a limit is free without bound (`u32::MAX`).
///
/// ```
/// use prometheus_parking_lot_core::semaphore::UnitVector;
/// use prometheus_parking_lot_core::{ResourceCost, ResourceKind};
///
/// let cost = ResourceCost::new(ResourceKind::GpuVram, 8).with(ResourceKind::Cpu, 1);
/// let demand = UnitVector::from_cost(&cost);
/// assert_eq!(demand.total(), 9);
/// assert_eq!(demand.of(ResourceKind::GpuVram), 8);
/// assert_eq!(UnitVector::scalar(3).of(ResourceKind::Cpu), 0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct UnitVector {
    total: u32,
    kinds: [u32; KIND_COUNT],
}

impl UnitVector {
    /// A demand of `units` in the total dimension and none in any kind: the
    /// single-dimension case.
    #[must_use]
    pub const fn scalar(units: u32) -> Self {
        Self {
            total: units,
            kinds: [0; KIND_COUNT],
        }
    }

    /// The demand of `cost`: its units per kind, and their sum (saturating,
    /// as [`ResourceCost::total_units`]) as the total.
    #[must_use]
    #[inline]
    pub fn from_cost(cost: &ResourceCost) -> Self {
        let mut demand = Self::default();
        for (kind, units) in cost.iter() {
            demand.kinds[kind_index(kind)] = units;
            demand.total = demand.total.saturating_add(units);
        }
        demand
    }

    /// Free capacity: `total` units in the total dimension and `kinds[i]` in
    /// the kind at index `i`.
    pub(crate) const fn free(total: u32, kinds: [u32; KIND_COUNT]) -> Self {
        Self { total, kinds }
    }

    /// Units in the total dimension.
    #[must_use]
    pub const fn total(&self) -> u32 {
        self.total
    }

    /// Whether the per-kind units sum past `u32::MAX`, so that
    /// [`from_cost`](Self::from_cost) saturated the total. Such a demand
    /// exceeds every pool's total.
    pub(crate) fn total_overflows(&self) -> bool {
        self.kinds
            .iter()
            .map(|&units| u64::from(units))
            .sum::<u64>()
            > u64::from(u32::MAX)
    }

    /// Units in dimension `kind`.
    #[must_use]
    pub const fn of(&self, kind: ResourceKind) -> u32 {
        self.kinds[kind_index(kind)]
    }

    /// Units per kind, in [`KIND_COUNT`] slots.
    pub(crate) const fn kinds(&self) -> &[u32; KIND_COUNT] {
        &self.kinds
    }

    /// Whether this demand fits in `free` in every dimension at once.
    #[must_use]
    #[inline]
    #[expect(
        clippy::needless_bitwise_bool,
        reason = "branch-free over the kinds, so the compare vectorizes"
    )]
    pub fn fits_in(&self, free: &Self) -> bool {
        // Non-short-circuit over the fixed-size kinds so it compiles to one
        // vector compare: this runs for every skip-ahead candidate.
        self.total <= free.total
            && self
                .kinds
                .iter()
                .zip(free.kinds.iter())
                .fold(true, |fits, (need, have)| fits & (need <= have))
    }
}

impl From<&ResourceCost> for UnitVector {
    fn from(cost: &ResourceCost) -> Self {
        Self::from_cost(cost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_index_matches_the_kinds_table() {
        for (index, kind) in KINDS.into_iter().enumerate() {
            assert_eq!(kind_index(kind), index);
        }
    }

    #[test]
    fn demand_of_a_cost_and_fit_per_dimension() {
        let demand = UnitVector::from_cost(
            &ResourceCost::new(ResourceKind::GpuVram, 8).with(ResourceKind::Cpu, 1),
        );
        assert_eq!(demand.total(), 9);
        let mut free = [u32::MAX; KIND_COUNT];
        free[kind_index(ResourceKind::GpuVram)] = 8;
        free[kind_index(ResourceKind::Cpu)] = 0;
        // 8 GiB free, 0 slots free: short in one dimension, so it does not fit.
        assert!(!demand.fits_in(&UnitVector::free(100, free)));
        free[kind_index(ResourceKind::Cpu)] = 1;
        assert!(demand.fits_in(&UnitVector::free(100, free)));
        assert!(!demand.fits_in(&UnitVector::free(8, free)));
    }
}
