//! Tests for `build_pools`, which builds resource pools from configuration.
//!
//! The `PoolBuilder` type these tests used to exercise was removed when the
//! builders were reduced to the `build_pools` function (cr-48); they now test
//! that function.

use std::cell::Cell;
use std::collections::HashMap;

use async_trait::async_trait;
use prometheus_parking_lot::builders::pool_builder::build_pools;
use prometheus_parking_lot::config::{
    MailboxBackendConfig, PoolConfig, QueueBackendConfig, RuntimeConfig, SchedulerConfig,
};
use prometheus_parking_lot::core::{SchedulerError, TaskError, TaskExecutor, TaskMetadata};
use prometheus_parking_lot::infra::{InMemoryMailbox, InMemoryQueue};
use prometheus_parking_lot::runtime::TokioSpawner;

#[derive(Clone)]
struct Echo;

#[async_trait]
impl TaskExecutor<String, String> for Echo {
    async fn execute(&self, payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        Ok(payload)
    }
}

const fn pool_config(max_units: u32) -> PoolConfig {
    PoolConfig {
        max_units,
        max_queue_depth: 50,
        default_timeout_secs: 60,
        queue: QueueBackendConfig::InMemory,
        mailbox: MailboxBackendConfig::InMemory,
        runtime: RuntimeConfig::Native,
    }
}

#[tokio::test]
async fn build_pools_builds_one_pool_per_configured_name() {
    let mut pools = HashMap::new();
    pools.insert("fast".to_string(), pool_config(10));
    pools.insert("slow".to_string(), pool_config(2));
    let cfg = SchedulerConfig { pools };

    let built = build_pools::<String, String, _, _, _, _, _, _, _>(
        &cfg,
        |_, c| Ok(InMemoryQueue::<String>::new(c.max_queue_depth)),
        |_, _| Ok(InMemoryMailbox::<String>::new()),
        |_, _| Ok(Echo),
        TokioSpawner::new(tokio::runtime::Handle::current()),
    )
    .expect("valid configuration builds");

    let mut names: Vec<_> = built.keys().cloned().collect();
    names.sort();
    assert_eq!(names, ["fast", "slow"]);
}

#[tokio::test]
async fn build_pools_rejects_invalid_config_before_calling_any_factory() {
    let mut pools = HashMap::new();
    pools.insert("broken".to_string(), pool_config(0));
    let cfg = SchedulerConfig { pools };

    let queue_calls = Cell::new(0_u32);
    let result = build_pools::<String, String, _, _, _, _, _, _, _>(
        &cfg,
        |_, c| {
            queue_calls.set(queue_calls.get() + 1);
            Ok(InMemoryQueue::<String>::new(c.max_queue_depth))
        },
        |_, _| Ok(InMemoryMailbox::<String>::new()),
        |_, _| Ok(Echo),
        TokioSpawner::new(tokio::runtime::Handle::current()),
    );

    assert!(matches!(result, Err(SchedulerError::InvalidConfig { .. })));
    assert_eq!(queue_calls.get(), 0, "validation runs before any factory");
}
