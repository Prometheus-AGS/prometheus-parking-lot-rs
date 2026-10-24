//! S4: `ResourcePool::with_name` and `with_resource_limit` are construction
//! steps. They work before the first `submit` and panic after it, even when
//! the pool is idle again (the release hook the first submission installs
//! keeps a weak reference to the pool's state), as their docs say.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use prometheus_parking_lot::core::{ResourcePool, TaskStatus};
use prometheus_parking_lot::infra::{InMemoryMailbox, InMemoryQueue};
use prometheus_parking_lot::runtime::TokioSpawner;
use prometheus_parking_lot::util::ResourceKind;

use crate::support::{Behaviour, Executor, eventually, limits, spawner, task};

type Pool = ResourcePool<
    String,
    String,
    InMemoryQueue<String>,
    InMemoryMailbox<String>,
    Executor,
    TokioSpawner,
>;

fn pool() -> Pool {
    ResourcePool::new(
        limits(4),
        InMemoryQueue::new(100),
        InMemoryMailbox::new(),
        Executor::new(Behaviour::Succeed),
        spawner(),
    )
}

/// A pool that ran one task and is idle again.
async fn idle_after_one_task() -> Pool {
    let pool = pool();
    let status = pool.submit(task("builders", "one")).await.expect("submit");
    assert!(
        matches!(status, TaskStatus::Running | TaskStatus::Queued),
        "{status:?}"
    );
    let pool_ref = &pool;
    let idle = eventually(Duration::from_secs(30), || async move {
        (pool_ref.completed_tasks() == 1 && pool_ref.running_tasks() == 0).then_some(())
    })
    .await;
    assert!(idle.is_some(), "the task never finished");
    pool
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pool_builders_work_before_the_first_submit() {
    let pool = pool()
        .with_name("built")
        .with_resource_limit(ResourceKind::GpuVram, 8);
    assert_eq!(pool.name(), "built");
    pool.submit(task("builders", "after-build"))
        .await
        .expect("a built pool accepts work");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pool_builders_panic_after_the_first_submit_even_when_idle() {
    let pool = idle_after_one_task().await;
    let renamed = catch_unwind(AssertUnwindSafe(move || pool.with_name("late")));
    assert!(renamed.is_err(), "with_name after a submit must panic");

    let pool = idle_after_one_task().await;
    let limited = catch_unwind(AssertUnwindSafe(move || {
        pool.with_resource_limit(ResourceKind::GpuVram, 8)
    }));
    assert!(
        limited.is_err(),
        "with_resource_limit after a submit must panic"
    );
}
