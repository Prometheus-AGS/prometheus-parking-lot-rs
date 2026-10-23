//! Queue and mailbox semantics as foreign code sees them: duplicates, capacity,
//! lease validation, payload fidelity.

mod common;

use parking_lot_ffi::{FfiError, NackDisposition, StoreConfig, TaskPriority, open_store};

use common::{config, task};

#[tokio::test]
async fn duplicate_idempotency_key_stores_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(config(&dir.path().join("q.redb")))
        .await
        .expect("open");

    let first = store
        .enqueue(task(1, "same", TaskPriority::Normal))
        .await
        .expect("enqueue");
    let second = store
        .enqueue(task(2, "same", TaskPriority::Normal))
        .await
        .expect("enqueue");
    assert!(!first.duplicate);
    assert!(second.duplicate);
    assert_eq!(second.entry_id, first.entry_id);
    assert_eq!(store.depth().await.expect("depth").live(), 1);
}

#[tokio::test]
async fn queue_full_is_a_typed_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(StoreConfig {
        max_depth: 1,
        ..config(&dir.path().join("q.redb"))
    })
    .await
    .expect("open");

    let _ = store
        .enqueue(task(1, "a", TaskPriority::Normal))
        .await
        .expect("enqueue");
    let full = store
        .enqueue(task(2, "b", TaskPriority::Normal))
        .await
        .expect_err("second enqueue exceeds max_depth");
    assert!(
        matches!(full, FfiError::QueueFull { capacity: 1, .. }),
        "{full:?}"
    );
}

#[tokio::test]
async fn zero_lease_is_rejected_and_extend_keeps_a_lease_alive() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(config(&dir.path().join("q.redb")))
        .await
        .expect("open");
    let _ = store
        .enqueue(task(1, "a", TaskPriority::Normal))
        .await
        .expect("enqueue");

    let zero = store.lease(1, 0).await.expect_err("zero lease");
    assert!(matches!(zero, FfiError::InvalidConfig { .. }), "{zero:?}");

    let item = store.lease(1, 10_000).await.expect("lease").remove(0);
    store
        .extend_lease(item.lease, 60_000)
        .await
        .expect("extend");
    store
        .nack(item.lease, NackDisposition::Requeue { delay_ms: 0 })
        .await
        .expect("nack");
    let again = store
        .ack(item.lease)
        .await
        .expect_err("a nacked lease is settled");
    assert!(matches!(again, FfiError::LeaseLost { .. }), "{again:?}");
}

/// Every byte value crosses the boundary and comes back unchanged.
#[tokio::test]
async fn payload_bytes_round_trip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = open_store(config(&dir.path().join("q.redb")))
        .await
        .expect("open");
    let mut new = task(1, "bytes", TaskPriority::Normal);
    new.payload = (0..=255_u8).collect();
    let _ = store.enqueue(new.clone()).await.expect("enqueue");

    let item = store.lease(1, 10_000).await.expect("lease").remove(0);
    assert_eq!(item.payload, new.payload);
}

#[tokio::test]
async fn opening_a_file_that_is_not_a_store_fails_with_backend() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("not-a-store.redb");
    std::fs::write(&path, b"this is not a redb database file").expect("write");

    let error = open_store(config(&path))
        .await
        .err()
        .expect("garbage file must not open");
    assert!(matches!(error, FfiError::Backend { .. }), "{error:?}");
}
