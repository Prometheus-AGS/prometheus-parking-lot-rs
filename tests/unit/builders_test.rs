//! Tests for builder modules

use prometheus_parking_lot::builders::pool_builder::PoolBuilder;
use prometheus_parking_lot::config::{PoolConfig, QueueBackendConfig, MailboxBackendConfig, RuntimeConfig};
use prometheus_parking_lot::util::serde::Priority;

#[test]
fn test_pool_builder_defaults() {
    let config = PoolConfig {
        max_units: 100,
        max_queue_depth: 50,
        default_timeout_secs: 60,
        queue: QueueBackendConfig::InMemory,
        mailbox: MailboxBackendConfig::InMemory,
        runtime: RuntimeConfig::Native,
    };

    let builder = PoolBuilder::new("pool1", config.clone());
    assert_eq!(builder.name(), "pool1");
    assert_eq!(builder.config().max_units, 100);
    assert_eq!(builder.config().max_queue_depth, 50);
}

#[test]
fn test_pool_builder_priority_default() {
    let priority = PoolBuilder::default_priority();
    assert_eq!(priority, Priority::Normal);
}
