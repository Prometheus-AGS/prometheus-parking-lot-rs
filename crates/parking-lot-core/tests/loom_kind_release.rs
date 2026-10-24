//! OA-11: bounded schedules through the production kind-limited release path.
//!
//! Mapping: try_acquire_vector -> claim_with_kinds checks kind counters and
//! claims the packed total/permit under the kind lock. Permit::drop -> release
//! -> release_with_kinds returns kind counters and packed state in one kind-lock
//! critical section. This target does not copy the semaphore state machine.
//!
//! The kind-lock unlock -> next lock pair publishes resource contents and
//! prevents lost kind updates. Packed Release decrement -> successful Acquire
//! CAS reading its release sequence supplies the resource-publication edge too.
//! The Relaxed resource atomic below deliberately supplies no independent edge.
//!
//! Final-boundary execution only: --cfg parking_lot_loom,
//! LOOM_MAX_PREEMPTIONS=3, isolated model target directory. Existing model
//! helper explores both yielded and non-yielded release windows. The
//! oa-11-kind-release control splits the actual kind release critical section;
//! only the parallel-return leak oracle should fail, while publication stays
//! a positive control. Loom's partial coverage of Relaxed accesses and these
//! bounded schedules do not prove every hardware reordering or replace runtime
//! tests.
#![cfg(parking_lot_loom)]

#[allow(dead_code, reason = "this target exercises raw semaphore permits")]
mod loom_support;

use loom::sync::Arc;
use loom::sync::atomic::{AtomicU32, Ordering};
use loom::thread;

use prometheus_parking_lot_core::semaphore::{UnitSemaphore, UnitVector};
use prometheus_parking_lot_core::{ResourceCost, ResourceKind};

fn demand(units: u32) -> UnitVector {
    UnitVector::from_cost(
        &ResourceCost::new(ResourceKind::GpuVram, units).with(ResourceKind::Cpu, units),
    )
}

#[test]
fn loom_kind_release_parallel_returns_preserve_all_kind_units() {
    loom_support::model(|| {
        let semaphore = Arc::new(
            UnitSemaphore::new(16)
                .with_resource_limit(ResourceKind::GpuVram, 2)
                .with_resource_limit(ResourceKind::Cpu, 2),
        );
        let first = UnitSemaphore::try_acquire_vector(&semaphore, &demand(1)).expect("first");
        let second = UnitSemaphore::try_acquire_vector(&semaphore, &demand(1)).expect("second");
        assert_eq!(semaphore.units_in_use(), 4);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(2));
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Cpu), Some(2));

        let left = thread::spawn(move || {
            loom_support::window_yield();
            drop(first);
        });
        let right = thread::spawn(move || {
            loom_support::window_yield();
            drop(second);
        });
        left.join().expect("left releaser");
        right.join().expect("right releaser");

        // Joins order both completed releases before these observations;
        // they do not order the two releases against each other.
        assert_eq!(semaphore.units_in_use(), 0, "OA-11 packed units leaked");
        assert_eq!(semaphore.permits_outstanding(), 0, "OA-11 permit leaked");
        assert_eq!(
            semaphore.units_in_use_of(ResourceKind::GpuVram),
            Some(0),
            "OA-11 kind release lost a GpuVram subtraction"
        );
        assert_eq!(
            semaphore.units_in_use_of(ResourceKind::Cpu),
            Some(0),
            "OA-11 kind release lost a Cpu subtraction"
        );
        let all = UnitSemaphore::try_acquire_vector(&semaphore, &demand(2))
            .expect("all kind capacity is reusable after both returns");
        assert_eq!(semaphore.units_in_use(), 4);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(2));
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Cpu), Some(2));
        drop(all);
        assert_eq!(semaphore.units_in_use(), 0);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(0));
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Cpu), Some(0));
    });
}

#[test]
fn loom_kind_release_reacquire_publishes_previous_holder_work() {
    loom_support::model(|| {
        let semaphore = Arc::new(
            UnitSemaphore::new(16)
                .with_resource_limit(ResourceKind::GpuVram, 1)
                .with_resource_limit(ResourceKind::Cpu, 1),
        );
        let resource = Arc::new(AtomicU32::new(0));
        let held = UnitSemaphore::try_acquire_vector(&semaphore, &demand(1)).expect("holder");
        let owner = {
            let resource = Arc::clone(&resource);
            thread::spawn(move || {
                // Relaxed models resource contents: the permit-release edge,
                // not this atomic, must publish the write to the next owner.
                resource.store(17, Ordering::Relaxed);
                loom_support::window_yield();
                drop(held);
            })
        };
        let waiter = {
            let semaphore = Arc::clone(&semaphore);
            let resource = Arc::clone(&resource);
            thread::spawn(move || {
                // One bounded attempt: None before return is legal. Success
                // means the kind lock and packed claim observed the return.
                if let Some(permit) = UnitSemaphore::try_acquire_vector(&semaphore, &demand(1)) {
                    assert_eq!(
                        resource.load(Ordering::Relaxed),
                        17,
                        "OA-11 kind reacquire missed the previous holder's work"
                    );
                    drop(permit);
                }
            })
        };
        owner.join().expect("previous owner");
        waiter.join().expect("possible next owner");
        assert_eq!(semaphore.units_in_use(), 0);
        assert_eq!(semaphore.permits_outstanding(), 0);
        assert_eq!(semaphore.units_in_use_of(ResourceKind::GpuVram), Some(0));
        assert_eq!(semaphore.units_in_use_of(ResourceKind::Cpu), Some(0));
    });
}
