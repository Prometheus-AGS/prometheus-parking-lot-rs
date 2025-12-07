//! Comprehensive benchmarks for the parking lot scheduler.
//!
//! Benchmarks cover:
//! - Queue operations (enqueue/dequeue/priority sorting)
//! - ResourcePool capacity management
//! - Task execution and wake-up mechanism
//! - Mailbox delivery
//! - End-to-end scheduling scenarios

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::hint::black_box;
use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot::core::{
    Mailbox, PoolLimits, ResourcePool, ScheduledTask, Spawn, TaskExecutor, TaskMetadata,
    TaskQueue, TaskStatus,
};
use prometheus_parking_lot::infra::mailbox::memory::InMemoryMailbox;
use prometheus_parking_lot::infra::queue::memory::InMemoryQueue;
use prometheus_parking_lot::util::clock::now_ms;
use prometheus_parking_lot::util::serde::{MailboxKey, Priority, ResourceCost, ResourceKind};

use async_trait::async_trait;
use tokio::runtime::Runtime;

// ============================================================================
// Test Payload and Executor
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
    async fn execute(&self, payload: BenchPayload, _meta: TaskMetadata) -> String {
        // Simulate minimal work
        format!("result-{}", payload.id)
    }
}

#[derive(Clone)]
struct NoOpSpawner;

impl Spawn for NoOpSpawner {
    fn spawn<F>(&self, fut: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        tokio::spawn(fut);
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

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
            cost: ResourceCost {
                kind: ResourceKind::Cpu,
                units: 1,
            },
            deadline_ms: None,
            created_at_ms: now_ms(),
        },
        payload: BenchPayload {
            id,
            data: format!("payload-data-{}", id),
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
            priority: if id % 2 == 0 {
                Priority::High
            } else {
                Priority::Normal
            },
            cost: ResourceCost {
                kind: ResourceKind::Cpu,
                units: 1,
            },
            deadline_ms: None,
            created_at_ms: 0,
        },
        payload: format!("payload-{}", id),
    }
}

// ============================================================================
// Queue Benchmarks
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
                
                // Enqueue tasks with mixed priorities
                for i in 0..size {
                    let priority = match i % 4 {
                        0 => Priority::Critical,
                        1 => Priority::High,
                        2 => Priority::Normal,
                        _ => Priority::Low,
                    };
                    q.enqueue(build_task(i, priority)).unwrap();
                }
                
                // Dequeue all tasks (they should come out sorted)
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
                
                // Enqueue tasks, half expired
                for i in 0..size {
                    let mut task = build_task(i, Priority::Normal);
                    if i % 2 == 0 {
                        task.meta.deadline_ms = Some(now - 1000); // Expired
                    }
                    q.enqueue(task).unwrap();
                }
                
                let pruned = q.prune_expired(now).unwrap();
                black_box(pruned);
            });
        });
    }
    group.finish();
}

// ============================================================================
// Mailbox Benchmarks
// ============================================================================

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
                        .deliver(&key, TaskStatus::Completed, Some(format!("result-{}", i)))
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
            let key = MailboxKey {
                tenant: "bench-tenant".into(),
                user_id: Some("bench-user".into()),
                session_id: None,
            };
            
            // Pre-populate mailbox
            for i in 0..size {
                mailbox
                    .deliver(&key, TaskStatus::Completed, Some(format!("result-{}", i)))
                    .unwrap();
            }
            
            b.iter(|| {
                let messages = mailbox.fetch(&key, None, size as usize);
                black_box(messages);
            });
        });
    }
    group.finish();
}

// ============================================================================
// ResourcePool Benchmarks (Async)
// ============================================================================

fn bench_pool_submit_immediate(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_submit_immediate");

    for capacity in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(capacity),
            &capacity,
            |b, &capacity| {
                b.to_async(Runtime::new().unwrap()).iter(|| async move {
                    let limits = PoolLimits {
                        max_units: capacity,
                        max_queue_depth: 1000,
                        default_timeout: Duration::from_secs(60),
                    };
                    
                    let queue = InMemoryQueue::new(1000);
                    let mailbox = InMemoryMailbox::new();
                    let executor = BenchExecutor;
                    let spawner = NoOpSpawner;
                    
                    let pool = Arc::new(ResourcePool::new(
                        limits,
                        queue,
                        mailbox,
                        executor,
                        spawner,
                    ));
                    
                    // Submit tasks that fit within capacity
                    for i in 0..capacity as u64 {
                        let task = build_task(i, Priority::Normal);
                        let status = pool.submit(task, now_ms()).await.unwrap();
                        black_box(status);
                    }
                    
                    // Small delay to let tasks start
                    tokio::time::sleep(Duration::from_millis(1)).await;
                });
            },
        );
    }
    group.finish();
}

fn bench_pool_submit_with_queueing(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_submit_with_queueing");

    for task_count in [50, 100, 200] {
        group.throughput(Throughput::Elements(task_count));
        group.bench_with_input(
            BenchmarkId::from_parameter(task_count),
            &task_count,
            |b, &task_count| {
                b.to_async(Runtime::new().unwrap()).iter(|| async move {
                    let limits = PoolLimits {
                        max_units: 10, // Small capacity to force queueing
                        max_queue_depth: 1000,
                        default_timeout: Duration::from_secs(60),
                    };
                    
                    let queue = InMemoryQueue::new(1000);
                    let mailbox = InMemoryMailbox::new();
                    let executor = BenchExecutor;
                    let spawner = NoOpSpawner;
                    
                    let pool = Arc::new(ResourcePool::new(
                        limits,
                        queue,
                        mailbox,
                        executor,
                        spawner,
                    ));
                    
                    // Submit more tasks than capacity
                    for i in 0..task_count {
                        let task = build_task(i, Priority::Normal);
                        let status = pool.submit(task, now_ms()).await.unwrap();
                        black_box(status);
                    }
                });
            },
        );
    }
    group.finish();
}

fn bench_pool_mixed_priorities(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_mixed_priorities");
    
    group.bench_function("mixed_priority_scheduling", |b| {
        b.to_async(Runtime::new().unwrap()).iter(|| async {
            let limits = PoolLimits {
                max_units: 20,
                max_queue_depth: 500,
                default_timeout: Duration::from_secs(60),
            };
            
            let queue = InMemoryQueue::new(500);
            let mailbox = InMemoryMailbox::new();
            let executor = BenchExecutor;
            let spawner = NoOpSpawner;
            
            let pool = Arc::new(ResourcePool::new(limits, queue, mailbox, executor, spawner));
            
            // Submit tasks with different priorities
            for i in 0..100u64 {
                let priority = match i % 4 {
                    0 => Priority::Critical,
                    1 => Priority::High,
                    2 => Priority::Normal,
                    _ => Priority::Low,
                };
                let task = build_task(i, priority);
                let status = pool.submit(task, now_ms()).await.unwrap();
                black_box(status);
            }
            
            // Allow some tasks to complete and wake others
            tokio::time::sleep(Duration::from_millis(10)).await;
        });
    });
    group.finish();
}

fn bench_pool_deadline_checking(c: &mut Criterion) {
    let mut group = c.benchmark_group("pool_deadline_checking");
    
    group.bench_function("reject_expired_tasks", |b| {
        b.to_async(Runtime::new().unwrap()).iter(|| async {
            let limits = PoolLimits {
                max_units: 10,
                max_queue_depth: 100,
                default_timeout: Duration::from_secs(60),
            };
            
            let queue = InMemoryQueue::new(100);
            let mailbox = InMemoryMailbox::new();
            let executor = BenchExecutor;
            let spawner = NoOpSpawner;
            
            let pool = Arc::new(ResourcePool::new(limits, queue, mailbox, executor, spawner));
            
            let now = now_ms();
            
            // Submit tasks with expired deadlines
            for i in 0..50u64 {
                let mut task = build_task(i, Priority::Normal);
                task.meta.deadline_ms = Some(now - 1000); // Already expired
                let result = pool.submit(task, now).await;
                let _ = black_box(result);
            }
        });
    });
    group.finish();
}

// ============================================================================
// End-to-End Scenario Benchmarks
// ============================================================================

fn bench_end_to_end_scenario(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end_scenario");
    
    group.bench_function("realistic_workload", |b| {
        b.to_async(Runtime::new().unwrap()).iter(|| async {
            let limits = PoolLimits {
                max_units: 25,
                max_queue_depth: 500,
                default_timeout: Duration::from_secs(60),
            };
            
            let queue = InMemoryQueue::new(500);
            let mailbox = InMemoryMailbox::new();
            let executor = BenchExecutor;
            let spawner = NoOpSpawner;
            
            let pool = Arc::new(ResourcePool::new(limits, queue, mailbox, executor, spawner));
            
            // Simulate realistic workload:
            // - Mix of priorities
            // - Some tasks start immediately, others queue
            // - Tasks complete and wake queued tasks
            for i in 0..150u64 {
                let priority = match i % 10 {
                    0..=1 => Priority::Critical,  // 20% critical
                    2..=4 => Priority::High,       // 30% high
                    5..=7 => Priority::Normal,     // 30% normal
                    _ => Priority::Low,            // 20% low
                };
                
                let mut task = build_task(i, priority);
                
                // 10% have deadlines
                if i % 10 == 0 {
                    task.meta.deadline_ms = Some(now_ms() + 5000);
                }
                
                let status = pool.submit(task, now_ms()).await.unwrap();
                black_box(status);
            }
            
            // Wait for tasks to process
            tokio::time::sleep(Duration::from_millis(20)).await;
        });
    });
    group.finish();
}

// ============================================================================
// Benchmark Groups
// ============================================================================

criterion_group!(
    queue_benches,
    bench_queue_enqueue_dequeue,
    bench_queue_priority_sorting,
    bench_queue_prune_expired
);

criterion_group!(
    mailbox_benches,
    bench_mailbox_deliver,
    bench_mailbox_fetch
);

criterion_group!(
    pool_benches,
    bench_pool_submit_immediate,
    bench_pool_submit_with_queueing,
    bench_pool_mixed_priorities,
    bench_pool_deadline_checking
);

criterion_group!(
    scenario_benches,
    bench_end_to_end_scenario
);

criterion_main!(queue_benches, mailbox_benches, pool_benches, scenario_benches);
