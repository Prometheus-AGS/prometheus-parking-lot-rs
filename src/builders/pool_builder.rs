//! Builders to construct resource pools from configuration.

use std::collections::HashMap;
use std::time::Duration;

use crate::config::{PoolConfig, SchedulerConfig};
use crate::core::{PoolLimits, ResourcePool, SchedulerError, TaskExecutor, TaskPayload};

/// Build resource pools from scheduler configuration using provided factories.
pub fn build_pools<P, T, Q, M, E, S, FQ, FM, FE>(
    cfg: &SchedulerConfig,
    mut queue_factory: FQ,
    mut mailbox_factory: FM,
    mut executor_factory: FE,
    spawner: S,
) -> Result<HashMap<String, ResourcePool<P, T, Q, M, E, S>>, SchedulerError>
where
    P: TaskPayload,
    T: Send + Sync + serde::Serialize + for<'de> serde::Deserialize<'de> + 'static,
    E: TaskExecutor<P, T>,
    FQ: FnMut(&str, &PoolConfig) -> Result<Q, SchedulerError>,
    FM: FnMut(&str, &PoolConfig) -> Result<M, SchedulerError>,
    FE: FnMut(&str, &PoolConfig) -> Result<E, SchedulerError>,
    S: Clone,
{
    cfg.validate()
        .map_err(|e| SchedulerError::Backend(format!("config invalid: {e}")))?;

    let mut pools = HashMap::new();
    for (name, pool_cfg) in &cfg.pools {
        let limits = PoolLimits {
            max_units: pool_cfg.max_units,
            max_queue_depth: pool_cfg.max_queue_depth,
            default_timeout: Duration::from_secs(pool_cfg.default_timeout_secs),
        };

        let queue = queue_factory(name, pool_cfg)?;
        let mailbox = mailbox_factory(name, pool_cfg)?;
        let executor = executor_factory(name, pool_cfg)?;
        let pool = ResourcePool::<P, T, Q, M, E, S>::new(limits, queue, mailbox, executor, spawner.clone());
        pools.insert(name.clone(), pool);
    }

    Ok(pools)
}
