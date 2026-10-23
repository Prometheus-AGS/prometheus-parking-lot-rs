//! The desktop example across an app restart (cr-44, "Desktop example" scenario):
//! submit, close the app, reopen, and the task is still there and is processed; a
//! result produced before a restart is still there to collect after it.
//!
//! This is the logic the Tauri commands call, run against a real store file; the
//! window is not involved.

use parking_lot_tauri_queue_service::{AppQueue, Collected, Status};

fn path(dir: &tempfile::TempDir) -> String {
    dir.path().join("queue.redb").to_string_lossy().into_owned()
}

#[tokio::test]
async fn submitted_task_is_processed_after_the_app_restarts() {
    let dir = tempfile::tempdir().expect("tempdir");

    let app = AppQueue::open(path(&dir)).await.expect("open");
    let task_id = app
        .submit("hello from before the restart")
        .await
        .expect("submit");
    assert_eq!(
        app.status().await.expect("status"),
        Status {
            waiting: 1,
            in_progress: 0
        }
    );
    drop(app); // the app quits

    let app = AppQueue::open(path(&dir)).await.expect("reopen");
    assert_eq!(
        app.status().await.expect("status"),
        Status {
            waiting: 1,
            in_progress: 0
        },
        "the task is still queued after the restart"
    );
    let processed = app
        .process_next()
        .await
        .expect("process")
        .expect("the task is leasable");
    assert_eq!(processed.task_id, task_id);
    assert_eq!(processed.attempt, 1);
    assert_eq!(
        app.collect().await.expect("collect"),
        vec![Collected {
            task_id,
            text: "HELLO FROM BEFORE THE RESTART".to_owned()
        }]
    );
    assert!(app.process_next().await.expect("process").is_none());
}

#[tokio::test]
async fn result_produced_before_a_restart_is_collected_after_it() {
    let dir = tempfile::tempdir().expect("tempdir");

    let app = AppQueue::open(path(&dir)).await.expect("open");
    let task_id = app
        .submit("finish while the window is closed")
        .await
        .expect("submit");
    let _ = app
        .process_next()
        .await
        .expect("process")
        .expect("leasable");
    drop(app); // the window closes before anyone reads the result

    let app = AppQueue::open(path(&dir)).await.expect("reopen");
    let results = app.collect().await.expect("collect");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].task_id, task_id);
    assert_eq!(results[0].text, "FINISH WHILE THE WINDOW IS CLOSED");
    assert!(
        app.collect().await.expect("collect").is_empty(),
        "a result is delivered once"
    );
}
