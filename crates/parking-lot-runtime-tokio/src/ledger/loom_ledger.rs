//! OA-11: bounded schedules through the actual private shutdown ledger.
//!
//! This is a library unit-test module inside ledger, not a path-included copy
//! in an integration crate. Ledger and its production ShutdownReport and
//! SinkOutcome types keep their actual defining-crate visibility, including
//! non-exhaustive construction and matching rules. No parallel DTOs are used.
//!
//! ResourcePool::submit calls Ledger::accept before loading its shutdown flag;
//! shutdown stores that flag before its wait predicate reads in_flight. The
//! first model preserves that program order and keeps accepted work unsettled
//! until both racing observations are recorded. Joins cannot repair an earlier
//! missed observation. SeqCst's total order excludes the store-buffering cycle;
//! it does not create a synchronizes-with edge between independent atomics.
//!
//! Loom 0.7.2 does not implement that cross-atomic SeqCst access order. This
//! exact admission litmus therefore uses a model-only supported SeqCst fence
//! between each store/load pair. Its two-fence order excludes the same
//! both-missed outcome; this is an explicit representation of this litmus's
//! SC constraint, not a fence present in production or a general C11 adapter.
//! The ordering control omits both model fences as it weakens the accesses.
//! The publication companion below uses no such adapter. See Loom's pinned
//! README Unsupported features and upstream issue180 for the tool limitation.
//!
//! The second model uses actual finish -> settle -> in_flight -> report calls.
//! The SeqCst settlement decrement is a release operation; an acquiring load
//! observing zero publishes the earlier Relaxed finished increment. The tally
//! does not provide an independent publication edge.
//!
//! No Tokio runtime is started. watching stays false, so neither Notify waits
//! nor notifications execute. This target does not model Notify internals,
//! cancellation capture, callbacks, or all ResourcePool behavior. Loom has
//! partial Relaxed-order coverage; bounded schedules are supporting evidence,
//! not a proof of all reorderings. Final-boundary execution only, with
//! LOOM_MAX_PREEMPTIONS=3 and isolated model target directories.
#![cfg(parking_lot_loom)]

use loom::sync::Arc;
use loom::sync::atomic::{AtomicBool, Ordering};
use loom::thread;

use super::{Ledger, Tally};

#[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
const FLAG_STORE_ORDER: Ordering = Ordering::SeqCst;
#[cfg(parking_lot_negative_control = "oa-11-ledger-order")]
const FLAG_STORE_ORDER: Ordering = Ordering::Release;
#[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
const FLAG_LOAD_ORDER: Ordering = Ordering::SeqCst;
#[cfg(parking_lot_negative_control = "oa-11-ledger-order")]
const FLAG_LOAD_ORDER: Ordering = Ordering::Acquire;

/// Represent the store/load SC constraint in this exact admission litmus.
/// Real production uses SeqCst accesses without this fence. Loom instead
/// applies acquire/release to those accesses, so the supported fence restores
/// the two-pair RAW constraint for this SC-only observation projection.
#[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
fn model_admission_sc_boundary() {
    loom::sync::atomic::fence(Ordering::SeqCst);
}

#[test]
fn loom_ledger_submit_shutdown_cannot_both_miss() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::default());
        let shutdown = Arc::new(AtomicBool::new(false));
        let submit = {
            let ledger = Arc::clone(&ledger);
            let shutdown = Arc::clone(&shutdown);
            thread::spawn(move || {
                ledger.accept();
                #[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
                model_admission_sc_boundary();
                if shutdown.load(FLAG_LOAD_ORDER) {
                    // The real rejected submission returns its accounting.
                    ledger.settle(1);
                    false
                } else {
                    true
                }
            })
        };
        let drain = {
            let ledger = Arc::clone(&ledger);
            thread::spawn(move || {
                shutdown.store(true, FLAG_STORE_ORDER);
                #[cfg(not(parking_lot_negative_control = "oa-11-ledger-order"))]
                model_admission_sc_boundary();
                ledger.in_flight()
            })
        };
        let accepted = submit.join().expect("submit observer");
        let seen = drain.join().expect("shutdown observer");
        assert!(
            !accepted || seen > 0,
            "OA-11 ledger admission and shutdown both missed accepted work"
        );
        if accepted {
            ledger.finish(1);
            ledger.settle(1);
        }
        assert_eq!(ledger.in_flight(), 0, "OA-11 submission accounting leaked");
    });
}

#[test]
fn loom_ledger_settlement_publishes_finished_before_zero() {
    loom::model(|| {
        let ledger = Arc::new(Ledger::default());
        ledger.accept();
        let settle = {
            let ledger = Arc::clone(&ledger);
            thread::spawn(move || {
                ledger.finish(1);
                ledger.settle(1);
            })
        };
        let observe = {
            let ledger = Arc::clone(&ledger);
            thread::spawn(move || {
                // A nonzero snapshot may wait again. Zero must publish the
                // finished tally before a report can treat the drain as done.
                if ledger.in_flight() == 0 {
                    assert_eq!(
                        ledger.report(Tally::default()).completed,
                        1,
                        "OA-11 settled zero did not publish the finished tally"
                    );
                }
            })
        };
        settle.join().expect("settler");
        observe.join().expect("report observer");
        assert_eq!(ledger.in_flight(), 0);
        assert_eq!(ledger.report(Tally::default()).completed, 1);
    });
}
