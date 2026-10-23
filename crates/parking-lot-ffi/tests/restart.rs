//! cr-44 / platform-targets: a task and a result survive an app restart, and a
//! hard kill. These tests drive the same `ParkingLotStore` the Tauri example calls
//! and the Kotlin and Swift bindings wrap.

mod common;

use std::process::Command;
use std::time::Duration;

use parking_lot_ffi::{
    DeliveryResult, FfiError, NackDisposition, ResultDelivery, TaskOutcome, TaskPriority,
    open_store,
};

use common::{config, mailbox, task};

/// Lease long enough that no test step outlives it.
const LONG_LEASE_MS: u64 = 60_000;
/// Lease short enough to wait out.
const SHORT_LEASE_MS: u64 = 50;

/// Exit code the child uses to say "enqueued, now killed".
const CHILD_EXIT_CODE: i32 = 86;

const CHILD_PATH_VAR: &str = "PARKING_LOT_FFI_CHILD_STORE";

/// Enqueue, drop the store (the app closes), reopen (the app restarts): the task is
/// still queued and leasable, byte for byte.
#[tokio::test]
async fn enqueued_task_survives_close_and_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.redb");

    let store = open_store(config(&path)).await.expect("open");
    let accepted = store
        .enqueue(task(7, "summarise", TaskPriority::Normal))
        .await
        .expect("enqueue");
    assert!(!accepted.duplicate);
    drop(store);

    let store = open_store(config(&path)).await.expect("reopen");
    let depth = store.depth().await.expect("depth");
    assert_eq!((depth.ready, depth.leased), (1, 0));

    let leased = store.lease(10, LONG_LEASE_MS).await.expect("lease");
    assert_eq!(leased.len(), 1);
    let item = &leased[0];
    assert_eq!(item.task_id, 7);
    assert_eq!(item.payload, b"summarise");
    assert_eq!(item.attempt, 1);
    assert_eq!(item.lease.entry_id, accepted.entry_id);
    assert_eq!(item.mailbox, Some(mailbox()));

    store.ack(item.lease).await.expect("ack");
    assert_eq!(store.depth().await.expect("depth").live(), 0);
}

/// A consumer that dies mid-task: its lease expires, the restarted app is handed the
/// task again with a higher attempt count, and the dead consumer's handle is fenced out.
#[tokio::test]
async fn leased_task_is_redelivered_after_restart_once_the_lease_expires() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.redb");

    let store = open_store(config(&path)).await.expect("open");
    let _ = store
        .enqueue(task(1, "work", TaskPriority::High))
        .await
        .expect("enqueue");
    let first = store
        .lease(1, SHORT_LEASE_MS)
        .await
        .expect("lease")
        .remove(0);
    assert_eq!(first.attempt, 1);
    drop(store); // the app dies holding the lease

    tokio::time::sleep(Duration::from_millis(SHORT_LEASE_MS * 3)).await;

    let store = open_store(config(&path)).await.expect("reopen");
    let second = store
        .lease(1, LONG_LEASE_MS)
        .await
        .expect("lease")
        .remove(0);
    assert_eq!(second.task_id, 1);
    assert_eq!(second.attempt, 2, "redelivery counts as a new attempt");
    assert_ne!(second.lease.fence, first.lease.fence);

    let stale = store.ack(first.lease).await.expect_err("stale lease");
    assert_eq!(
        stale,
        FfiError::LeaseLost {
            entry_id: first.lease.entry_id
        }
    );
    store.ack(second.lease).await.expect("ack current lease");
}

/// A result delivered while no client is connected is still there after a restart,
/// is collected exactly once, and a rerun of the task cannot store a second one.
#[tokio::test]
async fn result_waits_for_a_disconnected_client_across_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.redb");
    let delivery = |payload: &str| ResultDelivery {
        mailbox: mailbox(),
        task_id: 5,
        outcome: TaskOutcome::Completed,
        payload: Some(payload.as_bytes().to_vec()),
        idempotency_key: Some("job-5".to_owned()),
        ttl_ms: None,
    };

    let store = open_store(config(&path)).await.expect("open");
    assert_eq!(
        store
            .deliver_result(delivery("answer"))
            .await
            .expect("deliver"),
        DeliveryResult::Stored
    );
    drop(store);

    let store = open_store(config(&path)).await.expect("reopen");
    let peeked = store
        .peek_result(mailbox())
        .await
        .expect("peek")
        .expect("result survives restart");
    assert_eq!(peeked.payload.as_deref(), Some(&b"answer"[..]));

    let taken = store
        .consume_result(mailbox())
        .await
        .expect("consume")
        .expect("still there after peek");
    assert_eq!(taken.task_id, 5);
    assert_eq!(taken.outcome, TaskOutcome::Completed);
    assert!(
        store
            .consume_result(mailbox())
            .await
            .expect("consume")
            .is_none()
    );

    // The task is redelivered by its queue after the client took the result: the
    // key is remembered, so the rerun's result is not stored.
    assert_eq!(
        store
            .deliver_result(delivery("second"))
            .await
            .expect("deliver"),
        DeliveryResult::Duplicate
    );
    assert!(
        store
            .consume_result(mailbox())
            .await
            .expect("consume")
            .is_none()
    );
}

/// Priority order, dead letters and requeue delays all survive a restart.
#[tokio::test]
async fn queue_order_and_dead_letters_survive_restart() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("app.redb");

    let store = open_store(config(&path)).await.expect("open");
    for (id, name, priority) in [
        (1, "low", TaskPriority::Low),
        (2, "critical", TaskPriority::Critical),
        (3, "poison", TaskPriority::Normal),
        (4, "normal", TaskPriority::Normal),
    ] {
        let _ = store
            .enqueue(task(id, name, priority))
            .await
            .expect("enqueue");
    }
    let leased = store.lease(4, LONG_LEASE_MS).await.expect("lease");
    let poison = leased.iter().find(|l| l.task_id == 3).expect("poison");
    store
        .nack(
            poison.lease,
            NackDisposition::DeadLetter {
                reason: "unparseable".to_owned(),
            },
        )
        .await
        .expect("dead-letter");
    let normal = leased.iter().find(|l| l.task_id == 4).expect("normal");
    store
        .nack(normal.lease, NackDisposition::Requeue { delay_ms: 0 })
        .await
        .expect("requeue");
    drop(store);

    let store = open_store(config(&path)).await.expect("reopen");
    let depth = store.depth().await.expect("depth");
    assert_eq!(depth.dead_lettered, 1);
    // `normal` is ready again; `low` and `critical` are still leased until expiry.
    assert_eq!((depth.ready, depth.leased), (1, 2));
    let next = store.lease(1, LONG_LEASE_MS).await.expect("lease");
    assert_eq!(next[0].task_id, 4);
    assert_eq!(next[0].attempt, 2);
}

/// The hard-kill case: a separate process enqueues and exits without dropping the
/// store, so redb never closes the file (what the OS does to a killed app). The parent reopens the file
/// and finds the accepted task.
#[tokio::test]
async fn accepted_task_survives_a_killed_process() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("killed.redb");

    let status = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "child_enqueue_then_exit",
            "--ignored",
            "--nocapture",
        ])
        .env(CHILD_PATH_VAR, &path)
        .status()
        .expect("spawn child");
    assert_eq!(
        status.code(),
        Some(CHILD_EXIT_CODE),
        "the child must enqueue and then stop without closing the store"
    );

    let store = open_store(config(&path)).await.expect("reopen after kill");
    let leased = store.lease(10, LONG_LEASE_MS).await.expect("lease");
    assert_eq!(leased.len(), 1, "the acknowledged enqueue must not be lost");
    assert_eq!(leased[0].payload, b"accepted-before-kill");
}

/// Child half of `accepted_task_survives_a_killed_process`; never run on its own.
#[tokio::test]
#[ignore = "child process of accepted_task_survives_a_killed_process"]
async fn child_enqueue_then_exit() {
    let path = std::env::var_os(CHILD_PATH_VAR).expect("run through the parent test");
    let store = open_store(config(std::path::Path::new(&path)))
        .await
        .expect("open");
    let _ = store
        .enqueue(task(9, "accepted-before-kill", TaskPriority::Normal))
        .await
        .expect("enqueue is durable once it returns");
    // `exit` skips destructors: the store is never dropped, the file never closed.
    std::process::exit(CHILD_EXIT_CODE);
}
