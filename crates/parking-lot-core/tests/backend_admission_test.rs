//! OA-09: first-observed backend heads are checked against absolute limits.
//!
//! The production Dispatcher and TaskQueueAdapter read a pre-filled real
//! InMemoryQueue. `parking_lot_negative_control="oa-09"` skips the head check;
//! the impossible-head oracles then fail while valid-waiting controls remain.
#![cfg(not(parking_lot_loom))]

use prometheus_parking_lot_core::dispatch::{Dispatcher, TaskQueueAdapter};
use prometheus_parking_lot_core::semaphore::UnitSemaphore;
use prometheus_parking_lot_core::{
    InMemoryQueue, Priority, ResourceCost, ResourceKind, ScheduledTask, SchedulerError,
    TaskMetadata, TaskQueue,
};

fn task(id: u64, cost: ResourceCost) -> ScheduledTask<u64> {
    ScheduledTask {
        meta: TaskMetadata {
            id,
            mailbox: None,
            priority: Priority::Normal,
            cost,
            deadline_ms: None,
            created_at_ms: 0,
            idempotency_key: None,
            sequence: id,
        },
        payload: id,
    }
}

fn valid_cost() -> ResourceCost {
    ResourceCost::new(ResourceKind::GpuVram, 1).with(ResourceKind::Cpu, 1)
}

#[test]
fn backend_admission_rejects_total_and_kind_heads_once_before_fifo_followers() {
    let mut backend = InMemoryQueue::new(8);
    for item in [
        task(1, ResourceCost::new(ResourceKind::Cpu, 3)),
        task(2, ResourceCost::new(ResourceKind::GpuVram, 2)),
        task(3, valid_cost()),
        task(4, valid_cost()),
    ] {
        backend.enqueue(item).expect("prefill");
    }
    let dispatcher = Dispatcher::new(
        "oa-09",
        UnitSemaphore::new(2).with_resource_limit(ResourceKind::GpuVram, 1),
        8,
        TaskQueueAdapter::new(backend),
    )
    .with_max_head_skips(0);
    let observed = dispatcher.poke(0);
    assert_eq!(
        observed.rejected.len(),
        2,
        "both impossible heads leave once"
    );
    assert_eq!(observed.rejected[0].0.meta.id, 1);
    assert!(matches!(
        &observed.rejected[0].1,
        SchedulerError::CapacityExceeded {
            kind: None,
            requested: 3,
            available: 2,
            ..
        }
    ));
    assert_eq!(observed.rejected[1].0.meta.id, 2);
    assert!(matches!(
        &observed.rejected[1].1,
        SchedulerError::CapacityExceeded {
            kind: Some(ResourceKind::GpuVram),
            requested: 2,
            available: 1,
            ..
        }
    ));
    assert!(observed.expired.is_empty());
    assert_eq!(observed.started.len(), 1);
    let first = observed
        .started
        .into_iter()
        .next()
        .expect("first valid task");
    assert_eq!(first.item.meta.id, 3);
    assert_eq!(dispatcher.units_in_use(), 2);
    assert_eq!(dispatcher.units_in_use_of(ResourceKind::GpuVram), Some(1));
    assert_eq!(dispatcher.parked(), 1);
    assert!(
        dispatcher.poke(0).is_empty(),
        "valid successor waits without rejection"
    );
    let next = dispatcher.complete(first.permit, 0);
    assert!(next.rejected.is_empty(), "no rejection replay");
    assert!(next.expired.is_empty());
    assert_eq!(next.started.len(), 1);
    let second = next.started.into_iter().next().expect("second valid task");
    assert_eq!(second.item.meta.id, 4, "valid backend FIFO is preserved");
    assert_eq!(dispatcher.units_in_use(), 2);
    assert_eq!(dispatcher.units_in_use_of(ResourceKind::GpuVram), Some(1));
    assert!(dispatcher.complete(second.permit, 0).is_empty());
    assert_eq!((dispatcher.units_in_use(), dispatcher.parked()), (0, 0));
    assert_eq!(dispatcher.units_in_use_of(ResourceKind::GpuVram), Some(0));
    assert!(dispatcher.poke(0).rejected.is_empty());
}

#[test]
fn backend_admission_temporary_total_or_kind_pressure_is_not_rejection() {
    for kind_pressure in [false, true] {
        let (max_units, cost) = if kind_pressure {
            (4, ResourceCost::new(ResourceKind::GpuVram, 1))
        } else {
            (2, ResourceCost::new(ResourceKind::Cpu, 2))
        };
        let mut backend = InMemoryQueue::new(4);
        backend.enqueue(task(1, cost.clone())).expect("first");
        backend.enqueue(task(2, cost)).expect("second");
        let dispatcher = Dispatcher::new(
            "oa-09-valid",
            UnitSemaphore::new(max_units).with_resource_limit(ResourceKind::GpuVram, 1),
            4,
            TaskQueueAdapter::new(backend),
        )
        .with_max_head_skips(0);
        let mut first = dispatcher.poke(0);
        assert!(first.rejected.is_empty());
        assert_eq!(first.started.len(), 1);
        assert_eq!(dispatcher.parked(), 1);
        assert!(dispatcher.poke(0).is_empty());
        let grant = first.started.pop().expect("first task");
        assert_eq!(grant.item.meta.id, 1);
        let mut next = dispatcher.complete(grant.permit, 0);
        assert!(next.rejected.is_empty());
        assert_eq!(next.started.len(), 1);
        let grant = next.started.pop().expect("waiting task");
        assert_eq!(grant.item.meta.id, 2);
        assert!(dispatcher.units_in_use() <= max_units);
        assert!(
            dispatcher
                .units_in_use_of(ResourceKind::GpuVram)
                .expect("limit")
                <= 1
        );
        assert!(dispatcher.complete(grant.permit, 0).is_empty());
    }
}

#[test]
fn backend_admission_rejected_only_batch_requires_terminal_reporting() {
    let mut backend = InMemoryQueue::new(1);
    backend
        .enqueue(task(1, ResourceCost::new(ResourceKind::Cpu, 3)))
        .expect("prefill");
    let dispatcher = Dispatcher::new(
        "oa-09-rejected",
        UnitSemaphore::new(2),
        1,
        TaskQueueAdapter::new(backend),
    );
    let observed = dispatcher.poke(0);
    assert!(
        !observed.is_empty(),
        "rejected-only batches cannot be ignored"
    );
    assert_eq!(observed.rejected.len(), 1);
    assert!(observed.started.is_empty());
    assert_eq!(dispatcher.parked(), 0);
    assert!(dispatcher.poke(0).is_empty());
}

#[test]
fn backend_admission_direct_submit_keeps_its_capacity_error() {
    let dispatcher = Dispatcher::in_memory("oa-09-direct", UnitSemaphore::new(2), 4);
    assert!(matches!(
        dispatcher.submit(task(1, ResourceCost::new(ResourceKind::Cpu, 3)), 0),
        Err(SchedulerError::CapacityExceeded {
            kind: None,
            requested: 3,
            available: 2,
            ..
        })
    ));
    assert_eq!((dispatcher.units_in_use(), dispatcher.parked()), (0, 0));
}
