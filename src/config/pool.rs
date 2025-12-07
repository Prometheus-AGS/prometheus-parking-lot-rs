//! Pool and scheduler configuration structures.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Runtime adapter configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeConfig {
    /// Native runtime (e.g., Tokio).
    Native,
    /// Web worker or WASM adapter.
    WebWorker,
    /// Cloud worker adapter.
    CloudWorker,
}

/// Queue backend selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QueueBackendConfig {
    /// In-memory queue for development/testing.
    InMemory,
    /// File/embedded queue (e.g., Yaque).
    File,
    /// Postgres or pgmq-style queue.
    Postgres,
}

/// Mailbox backend selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailboxBackendConfig {
    /// In-memory mailbox.
    InMemory,
    /// File/embedded mailbox.
    File,
    /// Postgres mailbox.
    Postgres,
}

/// Pool configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Maximum concurrent resource units.
    pub max_units: u32,
    /// Maximum queued tasks before rejection.
    pub max_queue_depth: usize,
    /// Default timeout in seconds.
    pub default_timeout_secs: u64,
    /// Queue backend selection.
    pub queue: QueueBackendConfig,
    /// Mailbox backend selection.
    pub mailbox: MailboxBackendConfig,
    /// Runtime adapter selection.
    pub runtime: RuntimeConfig,
}

/// Root scheduler configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    /// Map of pool name to configuration.
    pub pools: HashMap<String, PoolConfig>,
}

impl PoolConfig {
    /// Validate pool configuration values.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_units == 0 {
            return Err("max_units must be greater than 0".into());
        }
        if self.max_queue_depth == 0 {
            return Err("max_queue_depth must be greater than 0".into());
        }
        if self.default_timeout_secs == 0 {
            return Err("default_timeout_secs must be greater than 0".into());
        }
        Ok(())
    }
}

impl SchedulerConfig {
    /// Validate all pools and ensure at least one pool exists.
    pub fn validate(&self) -> Result<(), String> {
        if self.pools.is_empty() {
            return Err("at least one pool must be defined".into());
        }
        for (name, pool) in &self.pools {
            pool.validate()
                .map_err(|e| format!("pool `{name}` invalid: {e}"))?;
        }
        Ok(())
    }

    /// Parse scheduler configuration from a JSON string and validate.
    pub fn from_json_str(input: &str) -> Result<Self, String> {
        let cfg: SchedulerConfig =
            serde_json::from_str(input).map_err(|e| format!("parse error: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }
}

/// Default number of worker threads (uses CPU count on native, 1 on WASM).
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
#[cfg(not(target_arch = "wasm32"))]
fn default_thread_stack_size() -> usize {
    2 * 1024 * 1024 // 2MB
}

/// Default maximum resource units.
fn default_max_units() -> u32 {
    1000
}

/// Default maximum queue depth.
fn default_max_queue_depth() -> usize {
    1000
}

/// Default timeout in milliseconds: 2 minutes.
fn default_timeout_ms() -> u64 {
    120_000
}

/// Configuration for the `WorkerPool`.
/// 
/// This configuration is used to create a worker pool with dedicated worker threads
/// (on native) or async tasks (on WASM). The same configuration works on all platforms,
/// with platform-specific fields handled via conditional compilation.
/// 
/// # Example
/// 
/// ```rust
/// use prometheus_parking_lot::config::WorkerPoolConfig;
/// 
/// let config = WorkerPoolConfig::new()
///     .with_worker_count(4)
///     .with_max_units(500)
///     .with_max_queue_depth(100)
///     .with_timeout_ms(60_000);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    #[cfg(not(target_arch = "wasm32"))]
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
    /// When the queue is full, new submissions return `PoolError::QueueFull`.
    #[serde(default = "default_max_queue_depth")]
    pub max_queue_depth: usize,
    
    /// Default timeout for `retrieve` operations in milliseconds.
    /// 
    /// If a result is not available within this time, `PoolError::Timeout` is returned.
    #[serde(default = "default_timeout_ms")]
    pub default_timeout_ms: u64,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            worker_count: default_worker_count(),
            #[cfg(not(target_arch = "wasm32"))]
            thread_stack_size: default_thread_stack_size(),
            max_units: default_max_units(),
            max_queue_depth: default_max_queue_depth(),
            default_timeout_ms: default_timeout_ms(),
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
    pub fn with_worker_count(mut self, count: usize) -> Self {
        self.worker_count = count;
        self
    }
    
    /// Set the thread stack size (native only, ignored on WASM).
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub fn with_thread_stack_size(mut self, size: usize) -> Self {
        self.thread_stack_size = size;
        self
    }
    
    /// Set the maximum resource units.
    #[must_use]
    pub fn with_max_units(mut self, units: u32) -> Self {
        self.max_units = units;
        self
    }
    
    /// Set the maximum queue depth.
    #[must_use]
    pub fn with_max_queue_depth(mut self, depth: usize) -> Self {
        self.max_queue_depth = depth;
        self
    }
    
    /// Set the default timeout in milliseconds.
    #[must_use]
    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.default_timeout_ms = timeout_ms;
        self
    }
    
    /// Get the default timeout as a `Duration`.
    #[must_use]
    pub fn default_timeout(&self) -> Duration {
        Duration::from_millis(self.default_timeout_ms)
    }
    
    /// Validate the configuration values.
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
        #[cfg(not(target_arch = "wasm32"))]
        if self.thread_stack_size < 64 * 1024 {
            return Err("thread_stack_size must be at least 64KB".into());
        }
        Ok(())
    }
}
