//! Tests for configuration validation

use prometheus_parking_lot::config::{PoolConfig, SchedulerConfig, RuntimeConfig, QueueBackendConfig, MailboxBackendConfig};

#[test]
fn test_pool_config_validation() {
    let valid = PoolConfig {
        max_units: 100,
        max_queue_depth: 50,
        default_timeout_secs: 60,
        queue: QueueBackendConfig::InMemory,
        mailbox: MailboxBackendConfig::InMemory,
        runtime: RuntimeConfig::Native,
    };
    assert!(valid.validate().is_ok());
}

#[test]
fn test_pool_config_invalid_max_units() {
    let invalid = PoolConfig {
        max_units: 0,
        max_queue_depth: 50,
        default_timeout_secs: 60,
        queue: QueueBackendConfig::InMemory,
        mailbox: MailboxBackendConfig::InMemory,
        runtime: RuntimeConfig::Native,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn test_pool_config_invalid_queue_depth() {
    let invalid = PoolConfig {
        max_units: 100,
        max_queue_depth: 0,
        default_timeout_secs: 60,
        queue: QueueBackendConfig::InMemory,
        mailbox: MailboxBackendConfig::InMemory,
        runtime: RuntimeConfig::Native,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn test_pool_config_invalid_timeout() {
    let invalid = PoolConfig {
        max_units: 100,
        max_queue_depth: 50,
        default_timeout_secs: 0,
        queue: QueueBackendConfig::InMemory,
        mailbox: MailboxBackendConfig::InMemory,
        runtime: RuntimeConfig::Native,
    };
    assert!(invalid.validate().is_err());
}

#[test]
fn test_scheduler_config_validation() {
    let mut pools = std::collections::HashMap::new();
    pools.insert("pool1".to_string(), PoolConfig {
        max_units: 100,
        max_queue_depth: 50,
        default_timeout_secs: 60,
        queue: QueueBackendConfig::InMemory,
        mailbox: MailboxBackendConfig::InMemory,
        runtime: RuntimeConfig::Native,
    });
    
    let config = SchedulerConfig { pools };
    assert!(config.validate().is_ok());
}

#[test]
fn test_scheduler_config_empty_pools() {
    let config = SchedulerConfig {
        pools: std::collections::HashMap::new(),
    };
    assert!(config.validate().is_err());
}

#[test]
fn test_scheduler_config_from_json() {
    let json = r#"{
        "pools": {
            "pool1": {
                "max_units": 100,
                "max_queue_depth": 50,
                "default_timeout_secs": 60,
                "queue": "in_memory",
                "mailbox": "in_memory",
                "runtime": "native"
            }
        }
    }"#;
    
    let config = SchedulerConfig::from_json_str(json);
    assert!(config.is_ok());
}
