//! Tests for error types

use prometheus_parking_lot::core::SchedulerError;

#[test]
fn test_queue_full_error() {
    let err = SchedulerError::QueueFull("test_pool".to_string());
    assert_eq!(format!("{}", err), "queue full: test_pool");
}

#[test]
fn test_capacity_exceeded_error() {
    let err = SchedulerError::CapacityExceeded;
    assert_eq!(format!("{}", err), "capacity exceeded");
}

#[test]
fn test_deadline_expired_error() {
    let err = SchedulerError::DeadlineExpired;
    assert_eq!(format!("{}", err), "deadline expired");
}

#[test]
fn test_backend_error() {
    let err = SchedulerError::Backend("connection failed".to_string());
    assert_eq!(format!("{}", err), "backend error: connection failed");
}
