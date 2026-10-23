//! Component costs of the scheduler (cr-40): the pieces every submission goes
//! through, measured one at a time on one thread.
//!
//! - `semaphore`: claim and release units on the `UnitSemaphore`, one
//!   dimension and with a per-kind limit (the cr-23 lock path).
//! - `dispatcher`: a submission that starts at once and its completion; a
//!   submission that parks and is woken by a completion.
//! - `queue_*`: `InMemoryQueue` enqueue/dequeue, priority ordering, expiry.
//! - `mailbox_*`: `InMemoryMailbox` delivery and lookup.
//! - `hook_emit`, `hook_pool_cycle`: lifecycle-hook overhead (cr-31), with no
//!   hook and with one no-op hook.
//!
//! End-to-end pool numbers are in the `throughput`, `latency` and
//! `utilization` suites.
//!
//! ```text
//! cargo bench --all-features --bench components
//! ```

#![expect(
    clippy::cast_possible_truncation,
    reason = "benchmark sizes are small constants; casts cannot truncate"
)]

#[allow(
    dead_code,
    reason = "each suite uses a different part of the shared module"
)]
mod support;

use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};

use prometheus_parking_lot::core::{
    Hook, HookBus, LifecycleEvent, Mailbox, PoolLimits, ResourcePool, ScheduledTask, Spawn,
    TaskError, TaskExecutor, TaskMetadata, TaskQueue, TaskStatus,
};
use prometheus_parking_lot::infra::mailbox::memory::InMemoryMailbox;
use prometheus_parking_lot::infra::queue::memory::InMemoryQueue;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};
use prometheus_parking_lot_core::dispatch::{Dispatcher, Parkable, Ticket};
use prometheus_parking_lot_core::semaphore::{UnitSemaphore, UnitVector};

// ============================================================================
// Payload, executor and spawner
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BenchPayload {
    id: u64,
    data: String,
}

#[derive(Clone)]
struct BenchExecutor;

#[async_trait]
impl TaskExecutor<BenchPayload, String> for BenchExecutor {
    async fn execute(
        &self,
        payload: BenchPayload,
        _meta: TaskMetadata,
    ) -> Result<String, TaskError> {
        Ok(format!("result-{}", payload.id))
    }
}

#[derive(Clone)]
struct TokioSpawn;

impl Spawn for TokioSpawn {
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}

fn build_task(id: u64, priority: Priority) -> ScheduledTask<BenchPayload> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: "bench-tenant".into(),
                user_id: Some(format!("user-{}", id % 10)),
                session_id: None,
            }),
            priority,
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: now_ms(),
            idempotency_key: None,
            sequence: 0,
        },
        payload: BenchPayload {
            id,
            data: format!("payload-data-{id}"),
        },
    }
}

fn build_string_task(id: u64) -> ScheduledTask<String> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: Some(MailboxKey {
                tenant: "tenant-1".into(),
                user_id: None,
                session_id: None,
            }),
            priority: if id.is_multiple_of(2) {
                Priority::High
            } else {
                Priority::Normal
            },
            cost: ResourceCost::new(ResourceKind::Cpu, 1),
            deadline_ms: None,
            created_at_ms: u128::from(id),
            idempotency_key: None,
            sequence: 0,
        },
        payload: format!("payload-{id}"),
    }
}

// ============================================================================
// Semaphore and dispatcher
// ============================================================================

/// Claim and release on the unit semaphore. `scalar`: one dimension, the
/// packed-CAS path. `per_kind_limit`: a demand on a kind with its own limit,
/// which takes the kind lock as well (cr-23).
fn bench_semaphore(c: &mut Criterion) {
    let mut group = c.benchmark_group("semaphore");
    let scalar = Arc::new(UnitSemaphore::new(16));
    group.bench_function("acquire_release/scalar", |b| {
        b.iter(|| black_box(UnitSemaphore::try_acquire(&scalar, black_box(1))));
    });
    let limited = Arc::new(UnitSemaphore::new(16).with_resource_limit(ResourceKind::GpuVram, 8));
    let demand = UnitVector::from_cost(
        &ResourceCost::new(ResourceKind::GpuVram, 2).with(ResourceKind::Cpu, 1),
    );
    group.bench_function("acquire_release/per_kind_limit", |b| {
        b.iter(|| {
            black_box(UnitSemaphore::try_acquire_vector(
                &limited,
                black_box(&demand),
            ))
        });
    });
    group.finish();
}

/// An item for the bare dispatcher.
#[derive(Debug)]
struct Item {
    sequence: u64,
}

impl Item {
    const fn new() -> Self {
        Self { sequence: 0 }
    }
}

impl Parkable for Item {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: Priority::Normal,
            sequence: self.sequence,
            demand: UnitVector::scalar(1),
            deadline_ms: None,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.sequence = sequence;
    }
}

/// `submit_complete`: a submission with free units starts at once; its
/// completion returns the permit (nothing parked). `park_wake`: with every
/// unit held, a submission parks; completing the held permit starts it, and
/// its permit is held for the next iteration.
fn bench_dispatcher(c: &mut Criterion) {
    let mut group = c.benchmark_group("dispatcher");
    let free = Dispatcher::in_memory("bench", UnitSemaphore::new(16), usize::MAX);
    group.bench_function("submit_complete", |b| {
        b.iter(|| {
            let admission = free.submit(Item::new(), 0).expect("unbounded depth");
            for grant in admission.dispatched.started {
                black_box(free.complete(grant.permit, 0).is_empty());
            }
        });
    });

    let full = Dispatcher::in_memory("bench", UnitSemaphore::new(1), usize::MAX);
    let first = full.submit(Item::new(), 0).expect("unbounded depth");
    let mut held = first
        .dispatched
        .started
        .into_iter()
        .next()
        .map(|g| g.permit);
    group.bench_function("park_wake", |b| {
        b.iter(|| {
            let admission = full.submit(Item::new(), 0).expect("unbounded depth");
            assert!(!admission.started, "every unit is held: the item parks");
            let permit = held.take().expect("a permit is held between iterations");
            held = full
                .complete(permit, 0)
                .started
                .into_iter()
                .next()
                .map(|g| g.permit);
        });
    });
    group.finish();
}

// ============================================================================
// Queue and mailbox
// ============================================================================

fn bench_queue_enqueue_dequeue(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_enqueue_dequeue");
    for size in [100, 1_000, 10_000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                let mut q = InMemoryQueue::new(size as usize);
                for i in 0..size {
                    q.enqueue(build_string_task(i)).unwrap();
                }
                while let Some(task) = q.dequeue().unwrap() {
                    black_box(task);
                }
            });
        });
    }
    group.finish();
}

fn bench_queue_priority_sorting(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_priority_sorting");
    for size in [100, 1_000, 5_000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                let mut q = InMemoryQueue::new(size as usize);
                for i in 0..size {
                    let priority = match i % 4 {
                        0 => Priority::Critical,
                        1 => Priority::High,
                        2 => Priority::Normal,
                        _ => Priority::Low,
                    };
                    q.enqueue(build_task(i, priority)).unwrap();
                }
                let mut count = 0;
                while q.dequeue().unwrap().is_some() {
                    count += 1;
                }
                black_box(count);
            });
        });
    }
    group.finish();
}

fn bench_queue_prune_expired(c: &mut Criterion) {
    let mut group = c.benchmark_group("queue_prune_expired");
    for size in [100, 1_000, 5_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                let mut q = InMemoryQueue::<BenchPayload>::new(size as usize);
                let now = now_ms();
                for i in 0..size {
                    let mut task = build_task(i, Priority::Normal);
                    if i % 2 == 0 {
                        task.meta.deadline_ms = Some(now - 1000);
                    }
                    q.enqueue(task).unwrap();
                }
                black_box(q.prune_expired(now).unwrap());
            });
        });
    }
    group.finish();
}

fn bench_mailbox_deliver(c: &mut Criterion) {
    let mut group = c.benchmark_group("mailbox_deliver");
    for size in [100, 1_000, 10_000] {
        group.throughput(Throughput::Elements(size));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| {
                let mut mailbox = InMemoryMailbox::<String>::new();
                let key = MailboxKey {
                    tenant: "bench-tenant".into(),
                    user_id: Some("bench-user".into()),
                    session_id: None,
                };
                for i in 0..size {
                    mailbox
                        .deliver(&key, TaskStatus::Completed, Some(format!("result-{i}")))
                        .unwrap();
                }
                black_box(mailbox);
            });
        });
    }
    group.finish();
}

fn bench_mailbox_fetch(c: &mut Criterion) {
    let mut group = c.benchmark_group("mailbox_fetch");
    for size in [100, 1_000, 5_000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let mut mailbox = InMemoryMailbox::<String>::new();
            // The mailbox holds at most one message per key, so populate
            // `size` distinct keys and look each one up.
            let keys: Vec<MailboxKey> = (0..size)
                .map(|i| MailboxKey {
                    tenant: "bench-tenant".into(),
                    user_id: Some(format!("bench-user-{i}")),
                    session_id: None,
                })
                .collect();
            for (i, key) in keys.iter().enumerate() {
                mailbox
                    .deliver(key, TaskStatus::Completed, Some(format!("result-{i}")))
                    .unwrap();
            }
            b.iter(|| {
                for key in &keys {
                    black_box(mailbox.peek(key));
                }
            });
        });
    }
    group.finish();
}

// ============================================================================
// Lifecycle-hook overhead (cr-31)
// ============================================================================

/// A hook that does nothing with the event.
struct NoopHook;

impl Hook for NoopHook {
    fn on_event(&self, event: &LifecycleEvent) {
        black_box(event.kind());
    }
}

/// One emission at a pool's transition, without and with a hook.
fn bench_hook_emit(c: &mut Criterion) {
    let mut group = c.benchmark_group("hook_emit");
    let started = |task_id| LifecycleEvent::Started {
        task_id,
        pool: "resource_pool".to_owned(),
        worker_id: 0,
        at_ms: now_ms(),
    };

    let bus = HookBus::new();
    group.bench_function("no_hooks", |b| {
        b.iter(|| bus.emit_with(|| started(black_box(1))));
    });

    let bus = HookBus::new();
    bus.register(Arc::new(NoopHook));
    group.bench_function("noop_hook", |b| {
        b.iter(|| bus.emit_with(|| started(black_box(1))));
    });
    group.finish();
}

/// `TASKS` submissions run to completion through a `ResourcePool`, without and
/// with one no-op hook.
fn bench_hook_pool_cycle(c: &mut Criterion) {
    const TASKS: u32 = 100;
    let mut group = c.benchmark_group("hook_pool_cycle");
    group.throughput(Throughput::Elements(u64::from(TASKS)));
    let runtime = support::runtime();
    for hooked in [false, true] {
        let name = if hooked { "noop_hook" } else { "no_hooks" };
        let mut pool = ResourcePool::new(
            PoolLimits {
                max_units: TASKS,
                max_queue_depth: 10 * TASKS as usize,
                default_timeout: Duration::from_secs(60),
            },
            InMemoryQueue::new(10 * TASKS as usize),
            InMemoryMailbox::new(),
            BenchExecutor,
            TokioSpawn,
        );
        if hooked {
            pool = pool.with_hook(Arc::new(NoopHook));
        }
        let pool = Arc::new(pool);
        let next_id = Arc::new(AtomicU32::new(0));
        group.bench_function(name, |b| {
            b.to_async(&runtime).iter(|| {
                let pool = Arc::clone(&pool);
                let next_id = Arc::clone(&next_id);
                async move {
                    let target = pool.completed_tasks() + u64::from(TASKS);
                    for _ in 0..TASKS {
                        let id = u64::from(next_id.fetch_add(1, Ordering::Relaxed));
                        black_box(pool.submit(build_task(id, Priority::Normal)).await.unwrap());
                    }
                    while pool.completed_tasks() < target {
                        tokio::task::yield_now().await;
                    }
                }
            });
        });
    }
    group.finish();
}

// ============================================================================
// Groups
// ============================================================================

criterion_group!(
    name = scheduling_benches;
    config = support::criterion_config();
    targets = bench_semaphore, bench_dispatcher
);

criterion_group!(
    name = queue_benches;
    config = support::criterion_config();
    targets = bench_queue_enqueue_dequeue, bench_queue_priority_sorting, bench_queue_prune_expired
);

criterion_group!(
    name = mailbox_benches;
    config = support::criterion_config();
    targets = bench_mailbox_deliver, bench_mailbox_fetch
);

criterion_group!(
    name = hook_benches;
    config = support::criterion_config();
    targets = bench_hook_emit, bench_hook_pool_cycle
);

criterion_main!(
    scheduling_benches,
    queue_benches,
    mailbox_benches,
    hook_benches
);
