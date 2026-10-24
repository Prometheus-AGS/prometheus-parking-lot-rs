//! Tuning of a [`DurableResourcePool`](super::DurableResourcePool): lease
//! duration, heartbeat and poll intervals, result TTL and the pool name.

use std::collections::BTreeMap;
use std::time::Duration;

use prometheus_parking_lot_core::{HookBus, PoolLimits, ResourceKind, SchedulerError};

/// Pool name used in errors, logs and lifecycle events.
pub(super) const DEFAULT_NAME: &str = "resource_pool";

/// Default interval between two `lease` calls while the queue is empty.
const DEFAULT_POLL: Duration = Duration::from_millis(500);

/// Default [`DurableOptions::oversize_attempt_limit`]: a task too large for
/// the pools that lease it is offered four times (one lease duration apart)
/// before the fifth lease settles it as dropped.
const DEFAULT_OVERSIZE_ATTEMPT_LIMIT: u32 = 5;

/// How a [`DurableResourcePool`](super::DurableResourcePool) leases, heartbeats
/// and polls.
///
/// Built from the lease duration; every other value has a default:
///
/// ```
/// use std::time::Duration;
/// use prometheus_parking_lot_runtime_tokio::DurableOptions;
///
/// let options = DurableOptions::new(Duration::from_secs(30))
///     .with_name("inference")
///     .with_poll_interval(Duration::from_millis(200));
/// assert_eq!(options.heartbeat_interval(), Duration::from_secs(10));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DurableOptions {
    lease_for: Duration,
    heartbeat_every: Duration,
    poll_every: Duration,
    result_ttl: Option<Duration>,
    name: String,
    hooks: HookBus,
    resource_limits: BTreeMap<ResourceKind, u32>,
    max_running: Option<u32>,
    oversize_attempt_limit: u32,
}

impl DurableOptions {
    /// Lease each task for `lease_for`, heartbeat every third of it, poll an
    /// empty queue every 500 ms, keep results until they are consumed, and name
    /// the pool `resource_pool`.
    #[must_use]
    pub fn new(lease_for: Duration) -> Self {
        Self {
            lease_for,
            heartbeat_every: lease_for / 3,
            poll_every: DEFAULT_POLL,
            result_ttl: None,
            name: DEFAULT_NAME.to_owned(),
            hooks: HookBus::new(),
            resource_limits: BTreeMap::new(),
            max_running: None,
            oversize_attempt_limit: DEFAULT_OVERSIZE_ATTEMPT_LIMIT,
        }
    }

    /// Extend each held lease this often. It must be shorter than the lease;
    /// the default, a third of it, survives one missed or slow beat.
    #[must_use]
    pub const fn with_heartbeat_interval(mut self, every: Duration) -> Self {
        self.heartbeat_every = every;
        self
    }

    /// Call `lease` this often while the queue is empty. Work submitted through
    /// this pool and capacity freed by its own tasks wake the dispatcher at
    /// once; the interval bounds how late it notices work enqueued by another
    /// process or a lease that expired.
    #[must_use]
    pub const fn with_poll_interval(mut self, every: Duration) -> Self {
        self.poll_every = every;
        self
    }

    /// Store each result with this time to live (the mailbox forgets the
    /// result's idempotency key when it expires). Without one, results are
    /// kept until consumed and their keys are kept for good.
    #[must_use]
    pub const fn with_result_ttl(mut self, ttl: Duration) -> Self {
        self.result_ttl = Some(ttl);
        self
    }

    /// Name the pool in errors, logs and lifecycle events.
    ///
    /// The name plays no part in exactly-one-result: a result delivered
    /// without a task idempotency key is keyed by the queue's identity and
    /// the entry (`"<queue identity>:entry:<entry id>"`, see
    /// [`LeaseQueue::identity`](prometheus_parking_lot_core::LeaseQueue::identity)),
    /// so any number of pools, named alike or not, can consume one queue, and
    /// pools on different queues can share one mailbox.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Emit the pool's lifecycle events (cr-31) on `hooks`. The bus is fixed
    /// before the pool starts leasing, so no event of a redelivered task is
    /// missed; register hooks on it before or after `start` (an empty bus
    /// costs nothing). Options compare equal only when they share one bus.
    #[must_use]
    pub fn with_hooks(mut self, hooks: HookBus) -> Self {
        self.hooks = hooks;
        self
    }

    /// Limit the units of dimension `kind` held by running tasks to `limit`
    /// (cr-23): a task starts only when its cost fits `max_units` and every
    /// limited kind at once, and a task over any limit is rejected at
    /// submission. A leased task over a limit (enqueued by a larger pool, or
    /// directly) is handed back for another consumer, up to the
    /// [oversize attempt limit](Self::with_oversize_attempt_limit).
    #[must_use]
    pub fn with_resource_limit(mut self, kind: ResourceKind, limit: u32) -> Self {
        self.resource_limits.insert(kind, limit);
        self
    }

    /// Run at most `max` tasks at once, whatever their cost. Units bound the
    /// tasks that cost something; this bound also holds for zero-cost tasks,
    /// which take no unit. Defaults to the pool's `max_units` (at least 1), so
    /// a zero-cost task counts like a one-unit task. Must be greater than
    /// zero.
    #[must_use]
    pub const fn with_max_running(mut self, max: u32) -> Self {
        self.max_running = Some(max);
        self
    }

    /// Bound how long a task too large for this pool keeps being handed back.
    ///
    /// A leased task whose cost exceeds this pool's `max_units` or a per-kind
    /// limit cannot run here, but another consumer of the same queue may fit
    /// it (mixed pool sizes, limits lowered during a rolling deploy), so the
    /// pool hands it back with `nack(Requeue)` delayed by one lease duration.
    /// When the task's attempt count (the queue's
    /// [`LeasedTask::attempt`](prometheus_parking_lot_core::LeasedTask), 1 on
    /// the first lease) has reached `limit`, the pool instead settles it: it
    /// delivers `TaskStatus::Dropped` to the task's mailbox key (if any),
    /// emits `Rejected` with `RejectionReason::CapacityExceeded`, and
    /// dead-letters the entry. A consumer that can run the task runs it at
    /// any attempt; the limit only applies to consumers that cannot.
    ///
    /// The attempt count includes every earlier lease of the entry (crash
    /// redeliveries and shutdown hand-backs too), so a task with that history
    /// reaches the limit after fewer oversize hand-backs. `1` drops such a
    /// task on its first lease. With a backend attempt limit (for example
    /// `PgLeaseQueue::with_max_attempts`), keep this at or below it, or the
    /// backend dead-letters the task first, without a mailbox delivery.
    /// Default: 5 (four hand-backs, one lease duration apart). Must be
    /// greater than zero.
    #[must_use]
    pub const fn with_oversize_attempt_limit(mut self, limit: u32) -> Self {
        self.oversize_attempt_limit = limit;
        self
    }

    /// The attempt at which a task too large for this pool is settled as
    /// dropped instead of handed back; see
    /// [`with_oversize_attempt_limit`](Self::with_oversize_attempt_limit).
    #[must_use]
    pub const fn oversize_attempt_limit(&self) -> u32 {
        self.oversize_attempt_limit
    }

    /// The bound set with [`with_max_running`](Self::with_max_running), if
    /// any.
    #[must_use]
    pub const fn max_running(&self) -> Option<u32> {
        self.max_running
    }

    /// The running-task bound in effect for a pool with `limits`.
    pub(super) fn max_running_for(&self, limits: &PoolLimits) -> u32 {
        self.max_running.unwrap_or_else(|| limits.max_units.max(1))
    }

    /// The per-kind limits, in `ResourceKind` order.
    #[must_use]
    pub const fn resource_limits(&self) -> &BTreeMap<ResourceKind, u32> {
        &self.resource_limits
    }

    /// The bus the pool emits lifecycle events on.
    #[must_use]
    pub const fn hooks(&self) -> &HookBus {
        &self.hooks
    }

    /// How long each lease lasts before it must be extended.
    #[must_use]
    pub const fn lease_duration(&self) -> Duration {
        self.lease_for
    }

    /// How often held leases are extended.
    #[must_use]
    pub const fn heartbeat_interval(&self) -> Duration {
        self.heartbeat_every
    }

    /// How often an idle dispatcher polls the queue.
    #[must_use]
    pub const fn poll_interval(&self) -> Duration {
        self.poll_every
    }

    /// The TTL results are stored with, if any.
    #[must_use]
    pub const fn result_ttl(&self) -> Option<Duration> {
        self.result_ttl
    }

    /// The pool's name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Reject values the dispatcher cannot run with.
    pub(super) fn validate(&self) -> Result<(), SchedulerError> {
        if self.lease_for.is_zero() {
            return Err(SchedulerError::invalid_config(
                "durable pool: the lease duration must be greater than zero",
            ));
        }
        if self.heartbeat_every.is_zero() || self.heartbeat_every >= self.lease_for {
            return Err(SchedulerError::invalid_config(format!(
                "durable pool: the heartbeat interval ({:?}) must be greater than zero and \
                 shorter than the lease ({:?})",
                self.heartbeat_every, self.lease_for
            )));
        }
        if self.poll_every.is_zero() {
            return Err(SchedulerError::invalid_config(
                "durable pool: the poll interval must be greater than zero",
            ));
        }
        if self.oversize_attempt_limit == 0 {
            return Err(SchedulerError::invalid_config(
                "durable pool: the oversize attempt limit must be greater than zero",
            ));
        }
        if self.max_running == Some(0) {
            return Err(SchedulerError::invalid_config(
                "durable pool: the running-task bound must be greater than zero",
            ));
        }
        if self.name.trim().is_empty() {
            return Err(SchedulerError::invalid_config(
                "durable pool: the name must not be empty",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_validation() {
        let options = DurableOptions::new(Duration::from_secs(3));
        assert_eq!(options.heartbeat_interval(), Duration::from_secs(1));
        assert!(options.validate().is_ok());
        assert!(DurableOptions::new(Duration::ZERO).validate().is_err());
        assert!(
            DurableOptions::new(Duration::from_secs(3))
                .with_heartbeat_interval(Duration::from_secs(3))
                .validate()
                .is_err()
        );
        assert!(
            DurableOptions::new(Duration::from_secs(3))
                .with_poll_interval(Duration::ZERO)
                .validate()
                .is_err()
        );
        assert!(
            DurableOptions::new(Duration::from_secs(3))
                .with_name(" ")
                .validate()
                .is_err()
        );
        assert!(
            DurableOptions::new(Duration::from_secs(3))
                .with_max_running(0)
                .validate()
                .is_err()
        );
        assert_eq!(
            options.oversize_attempt_limit(),
            DEFAULT_OVERSIZE_ATTEMPT_LIMIT
        );
        assert!(
            DurableOptions::new(Duration::from_secs(3))
                .with_oversize_attempt_limit(0)
                .validate()
                .is_err()
        );
    }
}
