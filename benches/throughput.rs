//! Dispatch throughput (cr-40): tasks per second through each pool.
//!
//! One criterion iteration submits a batch of tasks to a pool of 16 units and
//! waits until every result is readable (see `support/pools.rs` for what
//! "readable" means per pool). The executor does no work, so the number is the
//! scheduler's own cost per task: admission, parking, wake-up, execution
//! bookkeeping, result delivery, and for the durable pools the storage round
//! trips (persist, lease, deliver, ack).
//!
//! | case | pool | batch | costs |
//! |---|---|---|---|
//! | `resource_pool/unit` | `ResourcePool` | 1,000 | 1 unit each |
//! | `resource_pool/mixed` | `ResourcePool` | 1,000 | 1 to 4 units, fixed seed |
//! | `worker_pool/unit` | native `WorkerPool`, 16 threads | 1,000 | 1 unit each |
//! | `worker_pool/mixed` | native `WorkerPool`, 16 threads | 1,000 | 1 to 4 units, fixed seed |
//! | `durable_memory/unit` | `DurableResourcePool`, in-memory backends | 200 | 1 unit each |
//! | `durable_redb/unit` | `DurableResourcePool`, redb (`embedded` feature) | 50 | 1 unit each |
//!
//! A pool is built once per case and reused across iterations (warm caches,
//! steady state). The client runs on a 4-worker tokio runtime. The redb store
//! lives in a temporary directory (`$TMPDIR`) and commits durably (fsync), so
//! that case measures the disk as much as the scheduler.
//!
//! ```text
//! cargo bench --all-features --bench throughput
//! ```

#[path = "support/pools.rs"]
mod pools;
#[allow(
    dead_code,
    reason = "each suite uses a different part of the shared module"
)]
mod support;

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use criterion::{Criterion, SamplingMode, Throughput, criterion_group, criterion_main};

use pools::{Harness, Kind};
use support::{Lcg, Probe};

/// Units of every pool.
const UNITS: u32 = 16;
/// Seed of the mixed-cost sequence.
const SEED: u64 = 0x5eed;

/// Batch size of a case.
const fn batch(kind: Kind) -> u64 {
    match kind {
        Kind::Resource | Kind::Worker => 1_000,
        Kind::DurableMemory => 200,
        #[cfg(feature = "embedded")]
        Kind::DurableRedb => 50,
    }
}

/// The task costs of one batch: all 1, or 1 to 4 from the fixed seed.
fn costs(len: u64, mixed: bool) -> Arc<[u32]> {
    let mut rng = Lcg(SEED);
    (0..len)
        .map(|_| {
            if mixed {
                u32::try_from(1 + rng.next(4)).unwrap_or(1)
            } else {
                1
            }
        })
        .collect()
}

fn throughput(c: &mut Criterion) {
    let runtime = support::runtime();
    let mut group = c.benchmark_group("throughput");
    for &kind in Kind::ALL {
        let mixes: &[bool] = if kind.is_durable() {
            &[false]
        } else {
            &[false, true]
        };
        for &mixed in mixes {
            let batch = batch(kind);
            let costs = costs(batch, mixed);
            let probe = Arc::new(Probe::new(0));
            let harness = {
                let _guard = runtime.enter();
                Arc::new(Harness::new(
                    kind,
                    Arc::clone(&probe),
                    Duration::ZERO,
                    UNITS,
                ))
            };
            let next_id = Arc::new(AtomicU64::new(0));
            group.throughput(Throughput::Elements(batch));
            group.sampling_mode(if kind.is_durable() {
                SamplingMode::Flat
            } else {
                SamplingMode::Auto
            });
            let name = format!("{}/{}", kind.name(), if mixed { "mixed" } else { "unit" });
            group.bench_function(name, |b| {
                b.to_async(&runtime).iter(|| {
                    let harness = Arc::clone(&harness);
                    let probe = Arc::clone(&probe);
                    let costs = Arc::clone(&costs);
                    let next_id = Arc::clone(&next_id);
                    async move {
                        let target = probe.count() + batch;
                        for cost in costs.iter() {
                            // Relaxed: the counter only hands out unique ids.
                            let id = next_id.fetch_add(1, Ordering::Relaxed);
                            harness.submit(id, *cost).await;
                        }
                        probe.wait_for(target).await;
                    }
                });
            });
        }
    }
    group.finish();
}

criterion_group!(
    name = benches;
    config = support::criterion_config();
    targets = throughput
);
criterion_main!(benches);
