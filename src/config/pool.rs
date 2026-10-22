//! Pool and scheduler configuration structures.

use std::collections::HashMap;

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
    ///
    /// # Errors
    ///
    /// Returns a description of the first invalid field (zero units, queue
    /// depth or timeout).
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
    ///
    /// # Errors
    ///
    /// Returns a description when no pool is defined or any pool is invalid.
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
    ///
    /// # Errors
    ///
    /// Returns a description when the input is not valid JSON for this
    /// schema or the parsed configuration fails validation.
    pub fn from_json_str(input: &str) -> Result<Self, String> {
        let cfg: Self = serde_json::from_str(input).map_err(|e| format!("parse error: {e}"))?;
        cfg.validate()?;
        Ok(cfg)
    }
}

pub use prometheus_parking_lot_core::config::WorkerPoolConfig;
