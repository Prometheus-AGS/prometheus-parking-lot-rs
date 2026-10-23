//! Serialization-friendly core types: task identity, priority, resource cost
//! and mailbox routing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Unique task identifier.
pub type TaskId = u64;

/// Task priority for ordering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Lowest urgency.
    Low,
    /// Default urgency.
    Normal,
    /// Elevated urgency.
    High,
    /// Highest urgency.
    Critical,
}

/// A named capacity dimension.
///
/// A [`ResourceCost`] holds units per dimension. The set is
/// `#[non_exhaustive]`: new dimensions (for example worker slots) are added in
/// minor releases. The derived order is the fixed order in which a multi-resource
/// admitter acquires dimensions (cr-23), which rules out lock-order deadlocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResourceKind {
    /// CPU-bound work.
    Cpu,
    /// GPU VRAM-bound work.
    GpuVram,
    /// I/O-bound work.
    Io,
    /// Composite or custom resource.
    Mixed,
}

/// Resource cost of a task: a vector of units per [`ResourceKind`].
///
/// The common case is one dimension, built with [`ResourceCost::new`]:
///
/// ```
/// use prometheus_parking_lot_core::{ResourceCost, ResourceKind};
///
/// let cost = ResourceCost::new(ResourceKind::GpuVram, 4);
/// assert_eq!(cost.units_of(ResourceKind::GpuVram), 4);
/// assert_eq!(cost.total_units(), 4);
///
/// // A multi-resource task: 8 GiB of VRAM and one CPU slot.
/// let cost = ResourceCost::new(ResourceKind::GpuVram, 8).with(ResourceKind::Cpu, 1);
/// assert_eq!(cost.dimensions(), 2);
/// ```
///
/// Dimensions with zero units are not stored, so `new(kind, 0)` equals
/// [`ResourceCost::none`].
///
/// ## Admission
///
/// A pool admits a cost in every dimension at once (cr-23): its
/// [`total_units`](Self::total_units) against `max_units`, and its units of
/// each kind the pool limits against that kind's limit.
///
/// ## Serialization
///
/// A cost serializes as a map from dimension to units, for example
/// `{"gpu_vram": 8, "cpu": 1}`. Deserialization also accepts the 0.2 form
/// `{"kind": "gpu_vram", "units": 8}`, so metadata persisted by 0.2 backends
/// still loads.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(from = "ResourceCostRepr", into = "BTreeMap<ResourceKind, u32>")]
pub struct ResourceCost {
    dims: BTreeMap<ResourceKind, u32>,
}

impl ResourceCost {
    /// A one-dimension cost of `units` of `kind`.
    #[must_use]
    pub fn new(kind: ResourceKind, units: u32) -> Self {
        Self::none().with(kind, units)
    }

    /// A cost with no units in any dimension.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            dims: BTreeMap::new(),
        }
    }

    /// Set the units for `kind`, replacing any previous value; zero removes the
    /// dimension.
    #[must_use]
    pub fn with(mut self, kind: ResourceKind, units: u32) -> Self {
        if units == 0 {
            self.dims.remove(&kind);
        } else {
            self.dims.insert(kind, units);
        }
        self
    }

    /// Units required in dimension `kind` (zero when absent).
    #[must_use]
    pub fn units_of(&self, kind: ResourceKind) -> u32 {
        self.dims.get(&kind).copied().unwrap_or(0)
    }

    /// Sum of the units over every dimension, saturating at `u32::MAX`.
    ///
    /// Pools check it against `max_units`; for a one-dimension cost it is that
    /// dimension's units.
    #[must_use]
    pub fn total_units(&self) -> u32 {
        self.dims
            .values()
            .fold(0_u32, |sum, &units| sum.saturating_add(units))
    }

    /// Number of dimensions with a non-zero cost.
    #[must_use]
    pub fn dimensions(&self) -> usize {
        self.dims.len()
    }

    /// True when the cost is zero in every dimension.
    #[must_use]
    pub fn is_none(&self) -> bool {
        self.dims.is_empty()
    }

    /// The `(dimension, units)` pairs in dimension order.
    pub fn iter(&self) -> impl Iterator<Item = (ResourceKind, u32)> + '_ {
        self.dims.iter().map(|(&kind, &units)| (kind, units))
    }
}

impl From<(ResourceKind, u32)> for ResourceCost {
    fn from((kind, units): (ResourceKind, u32)) -> Self {
        Self::new(kind, units)
    }
}

impl FromIterator<(ResourceKind, u32)> for ResourceCost {
    /// Collect `(dimension, units)` pairs; a repeated dimension keeps its last
    /// value.
    fn from_iter<I: IntoIterator<Item = (ResourceKind, u32)>>(iter: I) -> Self {
        iter.into_iter()
            .fold(Self::none(), |cost, (kind, units)| cost.with(kind, units))
    }
}

impl From<ResourceCost> for BTreeMap<ResourceKind, u32> {
    fn from(cost: ResourceCost) -> Self {
        cost.dims
    }
}

/// Accepted wire forms of a [`ResourceCost`].
#[derive(Deserialize)]
#[serde(untagged)]
enum ResourceCostRepr {
    /// The 0.2 single-dimension form `{"kind": ..., "units": ...}`.
    Single(SingleCostRepr),
    /// The vector form `{"<kind>": units, ...}`.
    Vector(BTreeMap<ResourceKind, u32>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SingleCostRepr {
    kind: ResourceKind,
    units: u32,
}

impl From<ResourceCostRepr> for ResourceCost {
    fn from(repr: ResourceCostRepr) -> Self {
        match repr {
            ResourceCostRepr::Single(SingleCostRepr { kind, units }) => Self::new(kind, units),
            ResourceCostRepr::Vector(dims) => dims.into_iter().collect(),
        }
    }
}

/// Mailbox routing key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MailboxKey {
    /// Tenant identifier.
    pub tenant: String,
    /// Optional user identifier.
    pub user_id: Option<String>,
    /// Optional session identifier.
    pub session_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_dimension_cost_is_the_scalar_case() {
        let cost = ResourceCost::new(ResourceKind::GpuVram, 4);
        assert_eq!(cost.units_of(ResourceKind::GpuVram), 4);
        assert_eq!(cost.units_of(ResourceKind::Cpu), 0);
        assert_eq!(cost.total_units(), 4);
        assert_eq!(cost.dimensions(), 1);
        assert_eq!(
            ResourceCost::new(ResourceKind::Cpu, 0),
            ResourceCost::none()
        );
    }

    #[test]
    fn vector_cost_sums_and_saturates() {
        let cost = ResourceCost::new(ResourceKind::GpuVram, u32::MAX).with(ResourceKind::Cpu, 2);
        assert_eq!(cost.dimensions(), 2);
        assert_eq!(cost.total_units(), u32::MAX);
        let dims: Vec<_> = cost.iter().collect();
        assert_eq!(
            dims,
            vec![(ResourceKind::Cpu, 2), (ResourceKind::GpuVram, u32::MAX)]
        );
    }

    #[test]
    fn cost_serializes_as_a_map_and_reads_both_forms() {
        let cost = ResourceCost::new(ResourceKind::GpuVram, 8).with(ResourceKind::Cpu, 1);
        let json = serde_json::to_string(&cost).expect("serialize");
        assert_eq!(json, r#"{"cpu":1,"gpu_vram":8}"#);
        let back: ResourceCost = serde_json::from_str(&json).expect("vector form");
        assert_eq!(back, cost);

        let legacy: ResourceCost =
            serde_json::from_str(r#"{"kind":"gpu_vram","units":4}"#).expect("0.2 form");
        assert_eq!(legacy, ResourceCost::new(ResourceKind::GpuVram, 4));
    }
}
