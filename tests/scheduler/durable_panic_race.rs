//! S1: an executor that panics on its own while `shutdown_with` fires the
//! cancel token is a failure, not a cancellation. The pool used to decide by
//! reading the token after the panic was caught, so a real panic racing a
//! shutdown was handed back to the queue (and would run, and panic, again).

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use prometheus_parking_lot::core::{
    LeaseQueue, ResultMailbox, ShutdownPolicy, TaskError, TaskExecutor, TaskMetadata, TaskStatus,
};
use prometheus_parking_lot::infra::mailbox::InMemoryResultMailbox;
use prometheus_parking_lot::infra::queue::InMemoryLeaseQueue;
use prometheus_parking_lot::{DurableOptions, DurableResourcePool, LifecycleEvent};
use tokio::sync::Notify;

use crate::support::{event_log, key, limits, spawner, task};

/// How long the executor blocks inside its poll before panicking: the
/// shutdown below fires the cancel token within that window.
const BLOCK_FOR: Duration = Duration::from_secs(1);

/// Announces that it runs, blocks its worker thread (so the cancel token
/// fires while this very poll is in progress), then panics.
#[derive(Clone)]
struct PanicsWhileShutdownCancels {
    entered: Arc<Notify>,
}

#[async_trait]
impl TaskExecutor<String, String> for PanicsWhileShutdownCancels {
    async fn execute(&self, _payload: String, _meta: TaskMetadata) -> Result<String, TaskError> {
        self.entered.notify_one();
        std::thread::sleep(BLOCK_FOR);
        panic!("the executor's own panic");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn durable_executor_panic_racing_shutdown_fails_the_task() {
    let entered = Arc::new(Notify::new());
    let (bus, mut log) = event_log();
    let pool = DurableResourcePool::start(
        limits(1),
        InMemoryLeaseQueue::<String>::new(100),
        InMemoryResultMailbox::<String>::new(),
        PanicsWhileShutdownCancels {
            entered: Arc::clone(&entered),
        },
        spawner(),
        DurableOptions::new(Duration::from_secs(3))
            .with_name("racing")
            .with_poll_interval(Duration::from_millis(20))
            .with_hooks(bus),
    )
    .expect("start the pool");
    pool.submit(task("race", "race")).await.expect("submit");

    // The executor is inside its poll: cancel now. `CancelImmediately` fires
    // the token at once, well within `BLOCK_FOR`.
    entered.notified().await;
    let report = pool.shutdown_with(ShutdownPolicy::CancelImmediately).await;

    let terminal = log.wait_for(LifecycleEvent::is_terminal).await;
    assert!(
        matches!(terminal, Some(LifecycleEvent::Failed { task_id: 1, ref error, .. }) if error.contains("the executor's own panic")),
        "{terminal:?}; events: {:?}",
        log.kinds_of(1)
    );
    assert_eq!(
        (report.completed, report.requeued),
        (1, 0),
        "the panicked task settled as a failure, not handed back: {report:?}"
    );
    let entry = pool
        .mailbox()
        .consume(&key("race"))
        .await
        .expect("consume")
        .expect("the failure is delivered");
    assert!(
        matches!(&entry.status, TaskStatus::Failed(TaskError::ExecutorPanicked { message }) if message.contains("the executor's own panic")),
        "{:?}",
        entry.status
    );
    let depth = pool.queue().depth().await.expect("depth");
    assert_eq!((depth.live(), depth.dead_lettered), (0, 1), "{depth:?}");
}
