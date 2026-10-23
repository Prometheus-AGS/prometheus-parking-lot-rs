//! `WorkerPool` configuration.
//!
//! This type lives in the core crate because both worker pool implementations
//! (native in `prometheus-parking-lot-runtime-tokio`, WASM in
//! `prometheus-parking-lot-wasm`) are constructed from it.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::contracts::ResourceKind;

/// Default number of worker threads (uses CPU count on native, 1 on WASM).
#[cfg_attr(
    target_arch = "wasm32",
    expect(
        clippy::missing_const_for_fn,
        reason = "const only when num_cpus is not called"
    )
)]
fn default_worker_count() -> usize {
    #[cfg(not(target_arch = "wasm32"))]
    {
        num_cpus::get()
    }
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
}

/// Default thread stack size: 2MB.
const fn default_thread_stack_size() -> usize {
    2 * 1024 * 1024 // 2MB
}

/// Default maximum resource units.
const fn default_max_units() -> u32 {
    1000
}

/// Default maximum queue depth.
const fn default_max_queue_depth() -> usize {
    1000
}

/// Default skip-ahead bound (cr-20): the dispatcher's default.
const fn default_max_head_skips() -> u32 {
    crate::dispatch::DEFAULT_MAX_HEAD_SKIPS
}

/// Default execution budget (and native default result TTL) in milliseconds: 2 minutes.
const fn default_timeout_ms() -> u64 {
    120_000
}

/// Configuration for the `WorkerPool`.
///
/// This configuration is used to create a worker pool with dedicated worker threads
/// (on native) or async tasks (on WASM). The same configuration works on all platforms,
/// with platform-specific fields ignored where they do not apply.
///
/// Deserialization is strict: an unknown or misspelled field is an error that
/// names it. Every field exists on every target, so one configuration file
/// loads on native and WASM alike.
///
/// # Example
///
/// ```rust
/// use prometheus_parking_lot_core::config::WorkerPoolConfig;
///
/// let config = WorkerPoolConfig::new()
///     .with_worker_count(4)
///     .with_max_units(500)
///     .with_max_queue_depth(100)
///     .with_timeout_ms(60_000);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(not(parking_lot_negative_control = "s-22"), serde(deny_unknown_fields))]
pub struct WorkerPoolConfig {
    /// Number of worker threads (native) or concurrent async tasks (WASM).
    ///
    /// Default: `num_cpus::get()` on native, `1` on WASM.
    #[serde(default = "default_worker_count")]
    pub worker_count: usize,

    /// Stack size per worker thread in bytes (native only).
    ///
    /// This field is ignored on WASM targets.
    /// Default: 2MB (2 * 1024 * 1024 bytes).
    #[serde(default = "default_thread_stack_size")]
    pub thread_stack_size: usize,

    /// Maximum resource units that can be active concurrently.
    ///
    /// Tasks exceeding this limit are queued. Used for capacity-based
    /// admission control (e.g., GPU VRAM blocks, CPU slots).
    #[serde(default = "default_max_units")]
    pub max_units: u32,

    /// Maximum number of tasks that can be queued before rejection.
    ///
    /// When the queue is full, new submissions return `SchedulerError::QueueFull`.
    #[serde(default = "default_max_queue_depth")]
    pub max_queue_depth: usize,

    /// Execution budget in milliseconds for a task without a nearer deadline.
    ///
    /// A task still running when the budget runs out fails with
    /// `TaskError::Timeout` (delivered as `SchedulerError::Task` from
    /// `retrieve`). On native it is also the default time an unretrieved
    /// result is kept before eviction (`WorkerPool::with_result_ttl` overrides
    /// it); the WASM pool does not evict results.
    /// It does not bound `retrieve` itself: `retrieve` takes its own timeout.
    #[serde(default = "default_timeout_ms")]
    pub default_timeout_ms: u64,

    /// Skip-ahead bound (cr-20): how many times a parked task that does not
    /// fit the free units may be passed by a smaller parked task that does,
    /// before the pool reserves capacity for it and starts nothing else until
    /// it has started. `0` keeps strict head-of-line order.
    ///
    /// Default: 8 (`prometheus_parking_lot_core::dispatch::DEFAULT_MAX_HEAD_SKIPS`).
    #[serde(default = "default_max_head_skips")]
    pub max_head_skips: u32,

    /// Per-dimension limits (cr-23): units each [`ResourceKind`] may hold at
    /// once, e.g. `{"gpu_vram": 24, "cpu": 4}`. A task is admitted only when
    /// its cost fits `max_units` (the total over every kind), the worker
    /// count and every limit here at once; a kind not listed is bounded only
    /// by `max_units`. A task over any limit is rejected with
    /// `CapacityExceeded`.
    ///
    /// Optional; empty by default, which is single-dimension admission by
    /// `max_units` alone. Each limit must be greater than zero and at most
    /// `max_units` (a larger one could never be reached; raise `max_units`).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resource_limits: BTreeMap<ResourceKind, u32>,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            worker_count: default_worker_count(),
            thread_stack_size: default_thread_stack_size(),
            max_units: default_max_units(),
            max_queue_depth: default_max_queue_depth(),
            default_timeout_ms: default_timeout_ms(),
            max_head_skips: default_max_head_skips(),
            resource_limits: BTreeMap::new(),
        }
    }
}

impl WorkerPoolConfig {
    /// Create a new configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of worker threads/tasks.
    #[must_use]
    pub const fn with_worker_count(mut self, count: usize) -> Self {
        self.worker_count = count;
        self
    }

    /// Set the thread stack size (native only, ignored on WASM).
    #[must_use]
    pub const fn with_thread_stack_size(mut self, size: usize) -> Self {
        self.thread_stack_size = size;
        self
    }

    /// Set the maximum resource units.
    #[must_use]
    pub const fn with_max_units(mut self, units: u32) -> Self {
        self.max_units = units;
        self
    }

    /// Set the maximum queue depth.
    #[must_use]
    pub const fn with_max_queue_depth(mut self, depth: usize) -> Self {
        self.max_queue_depth = depth;
        self
    }

    /// Set the execution budget (and native default result TTL) in milliseconds; see
    /// [`default_timeout_ms`](Self::default_timeout_ms).
    #[must_use]
    pub const fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.default_timeout_ms = timeout_ms;
        self
    }

    /// Set the skip-ahead bound; see
    /// [`max_head_skips`](Self::max_head_skips). `0` turns skip-ahead off.
    #[must_use]
    pub const fn with_max_head_skips(mut self, max_head_skips: u32) -> Self {
        self.max_head_skips = max_head_skips;
        self
    }

    /// Limit dimension `kind` to `limit` units; see
    /// [`resource_limits`](Self::resource_limits).
    #[must_use]
    pub fn with_resource_limit(mut self, kind: ResourceKind, limit: u32) -> Self {
        self.resource_limits.insert(kind, limit);
        self
    }

    /// The execution budget (and native default result TTL) as a `Duration`.
    #[must_use]
    pub const fn default_timeout(&self) -> Duration {
        Duration::from_millis(self.default_timeout_ms)
    }

    /// Validate the configuration values.
    ///
    /// # Errors
    ///
    /// Returns a description of the first invalid field.
    pub fn validate(&self) -> Result<(), String> {
        if self.worker_count == 0 {
            return Err("worker_count must be greater than 0".into());
        }
        if self.max_units == 0 {
            return Err("max_units must be greater than 0".into());
        }
        if self.max_queue_depth == 0 {
            return Err("max_queue_depth must be greater than 0".into());
        }
        if self.default_timeout_ms == 0 {
            return Err("default_timeout_ms must be greater than 0".into());
        }
        for (kind, &limit) in &self.resource_limits {
            if limit == 0 {
                return Err(format!(
                    "resource_limits[{kind:?}] must be greater than 0 (omit the kind to leave it \
                     bounded by max_units alone)"
                ));
            }
            if limit > self.max_units {
                return Err(format!(
                    "resource_limits[{kind:?}] ({limit}) exceeds max_units ({}); raise max_units \
                     to at least the largest kind limit",
                    self.max_units
                ));
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.thread_stack_size < 64 * 1024 {
            return Err("thread_stack_size must be at least 64KB".into());
        }
        Ok(())
    }
}
