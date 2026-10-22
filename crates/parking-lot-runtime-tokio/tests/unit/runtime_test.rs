//! Tests for tokio spawner utilities

use prometheus_parking_lot_runtime_tokio::tokio_spawner::TokioSpawner;
use prometheus_parking_lot_core::Spawn;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_tokio_spawner_spawn() {
    let spawner = TokioSpawner::new(tokio::runtime::Handle::current());

    let (tx, rx) = tokio::sync::oneshot::channel();
    spawner.spawn(async move {
        tx.send(123).unwrap();
    });

    let result = rx.await.expect("oneshot result");
    assert_eq!(result, 123);
}
