//! Tests for the scheduler error types' messages and retry classification.

use prometheus_parking_lot_core::{ResourceKind, SchedulerError};

#[test]
fn queue_full_message_names_pool_and_depth() {
    let err = SchedulerError::queue_full("test_pool", 5, 5);
    assert_eq!(
        err.to_string(),
        "queue full for pool 'test_pool': depth 5/5"
    );
    assert!(err.is_retryable());
}

#[test]
fn capacity_exceeded_message_names_request_and_pool() {
    let err = SchedulerError::capacity_exceeded("gpu", None, 9, 8);
    assert_eq!(
        err.to_string(),
        "capacity exceeded: need 9 units, pool 'gpu' has 8"
    );
    assert!(!err.is_retryable());
    let err = SchedulerError::capacity_exceeded("gpu", Some(ResourceKind::Cpu), 3, 2);
    assert_eq!(
        err.to_string(),
        "capacity exceeded: need 3 cpu units, pool 'gpu' has 2"
    );
}

#[test]
fn deadline_expired_message_names_task() {
    let err = SchedulerError::DeadlineExpired { task_id: 7 };
    assert_eq!(err.to_string(), "deadline expired for task 7");
}

#[test]
fn backend_error_keeps_its_source() {
    use std::error::Error;

    let err = SchedulerError::backend("connect", std::io::Error::other("connection failed"));
    assert_eq!(
        err.to_string(),
        "backend error in 'connect': connection failed"
    );
    assert!(err.source().is_some());
}
