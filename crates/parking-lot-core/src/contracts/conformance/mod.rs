//! Backend conformance suite for [`LeaseQueue`] and [`ResultMailbox`]
//! (feature `conformance`; ADR 0003).
//!
//! Every queue and mailbox backend runs this one suite in its own tests, so
//! all backends keep the same lease, ordering, idempotency and TTL rules. The
//! suite is runtime-agnostic: each case is an `async fn` generic over a
//! *harness* that builds fresh backends and moves the backend's clock.
//!
//! # Plugging in a backend
//!
//! 1. Enable the feature for tests:
//!    `prometheus-parking-lot-core = { workspace = true, features = ["conformance"] }`
//!    under `[dev-dependencies]`.
//! 2. Implement [`QueueHarness`] and/or [`MailboxHarness`]. `queue`/`mailbox`
//!    must return an empty, isolated backend on every call (for example a
//!    fresh queue name or schema); `advance` must move the backend's clock
//!    forward by at least the given duration (a `TestClock` advance, or a real
//!    sleep for backends that read database time, with
//!    [`QueueHarness::lease_duration`] lowered to keep the suite fast).
//! 3. Generate one test per case:
//!
//! ```ignore
//! prometheus_parking_lot_core::lease_queue_conformance!(
//!     test_attr = #[tokio::test],
//!     harness = PgQueueHarness::start().await,
//! );
//! prometheus_parking_lot_core::result_mailbox_conformance!(
//!     test_attr = #[tokio::test],
//!     harness = PgMailboxHarness::start().await,
//! );
//! ```
//!
//! A synchronous executor works too: `runner = futures::executor::block_on`
//! in place of `test_attr = ...` generates `#[test]` functions that run each
//! case on it (this is how the in-memory reference backends are checked).
//! [`queue::run_all`] and [`mailbox::run_all`] run every case in one test.

#![allow(
    clippy::missing_panics_doc,
    reason = "conformance cases are test code: a panic is a failed case"
)]

use std::future::Future;
use std::time::Duration;

use crate::contracts::clock::TestClock;
use crate::contracts::lease_queue::LeaseQueue;
use crate::contracts::memory_lease_queue::InMemoryLeaseQueue;
use crate::contracts::memory_result_mailbox::InMemoryResultMailbox;
use crate::contracts::result_mailbox::ResultMailbox;

pub mod mailbox;
#[cfg(test)]
mod negative;
pub mod queue;

/// Builds isolated [`LeaseQueue`] backends and moves their clock.
///
/// Payloads in the suite are `String`s.
pub trait QueueHarness: Send + Sync {
    /// The backend under test.
    type Queue: LeaseQueue<String>;

    /// A new, empty queue that accepts at most `max_depth` live entries and
    /// shares nothing with queues returned earlier.
    fn queue(&self, max_depth: usize) -> impl Future<Output = Self::Queue> + Send;

    /// Move the clock the backend times leases with forward by at least `by`.
    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send;

    /// Lease duration the cases use; every `advance` is a fraction or small
    /// multiple of it. Backends on real time lower it (for example to 1 s).
    fn lease_duration(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// Builds isolated [`ResultMailbox`] backends and moves their clock.
///
/// Payloads in the suite are `String`s.
pub trait MailboxHarness: Send + Sync {
    /// The backend under test.
    type Mailbox: ResultMailbox<String>;

    /// A new, empty mailbox that shares nothing with mailboxes returned
    /// earlier.
    fn mailbox(&self) -> impl Future<Output = Self::Mailbox> + Send;

    /// Move the clock the backend times TTLs with forward by at least `by`.
    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send;

    /// TTL the cases use. Backends on real time lower it (for example to 1 s).
    fn ttl(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// Harness for the in-memory reference queue, timed by a [`TestClock`].
#[derive(Debug, Clone, Default)]
pub struct InMemoryQueueHarness {
    clock: TestClock,
}

impl InMemoryQueueHarness {
    /// A harness with its own test clock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl QueueHarness for InMemoryQueueHarness {
    type Queue = InMemoryLeaseQueue<String, TestClock>;

    fn queue(&self, max_depth: usize) -> impl Future<Output = Self::Queue> + Send {
        std::future::ready(InMemoryLeaseQueue::with_clock(
            max_depth,
            self.clock.clone(),
        ))
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.clock.advance(by);
        std::future::ready(())
    }
}

/// Harness for the in-memory reference mailbox, timed by a [`TestClock`].
#[derive(Debug, Clone, Default)]
pub struct InMemoryMailboxHarness {
    clock: TestClock,
}

impl InMemoryMailboxHarness {
    /// A harness with its own test clock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl MailboxHarness for InMemoryMailboxHarness {
    type Mailbox = InMemoryResultMailbox<String, TestClock>;

    fn mailbox(&self) -> impl Future<Output = Self::Mailbox> + Send {
        std::future::ready(InMemoryResultMailbox::with_clock(self.clock.clone()))
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.clock.advance(by);
        std::future::ready(())
    }
}

/// Expands to one test function per case of a conformance module.
#[doc(hidden)]
#[macro_export]
macro_rules! __parking_lot_conformance_cases {
    (
        module = $module:ident,
        cases = [$($case:ident),* $(,)?],
        runner = $run:path,
        harness = $harness:expr $(,)?
    ) => {
        $(
            #[test]
            fn $case() {
                $run(async {
                    let harness = $harness;
                    $crate::contracts::conformance::$module::$case(&harness).await;
                });
            }
        )*
    };
    (
        module = $module:ident,
        cases = [$($case:ident),* $(,)?],
        test_attr = #[$attr:meta],
        harness = $harness:expr $(,)?
    ) => {
        $(
            #[$attr]
            async fn $case() {
                let harness = $harness;
                $crate::contracts::conformance::$module::$case(&harness).await;
            }
        )*
    };
}

/// Generates one test per [`LeaseQueue`] conformance case.
///
/// Takes either `runner = <path to a block_on fn>, harness = <expr>` (plain
/// `#[test]` functions that run each case on that executor) or
/// `test_attr = #[<async test attribute>], harness = <expr>` (for example
/// `#[tokio::test]`). `harness` is evaluated afresh in every test, inside the
/// async body, so it may `.await`. See the
/// [`conformance`](crate::contracts::conformance) module.
#[macro_export]
macro_rules! lease_queue_conformance {
    ($($args:tt)*) => {
        $crate::__parking_lot_conformance_cases!(
            module = queue,
            cases = [
                enqueue_and_lease_round_trip,
                leased_task_is_invisible,
                lease_expiry_redelivers,
                expired_lease_cannot_be_used,
                ack_removes_task,
                nack_requeue_redelivers,
                nack_requeue_with_delay,
                nacked_token_is_fenced_during_delay,
                nack_dead_letter,
                extend_keeps_lease,
                priority_then_fifo,
                requeued_task_keeps_its_place,
                lease_arguments,
                max_depth_counts_live_entries,
                idempotent_enqueue,
                identity_is_stable_and_distinct,
            ],
            $($args)*
        );
    };
}

/// Generates one test per [`ResultMailbox`] conformance case; arguments as
/// for [`lease_queue_conformance!`](crate::lease_queue_conformance).
#[macro_export]
macro_rules! result_mailbox_conformance {
    ($($args:tt)*) => {
        $crate::__parking_lot_conformance_cases!(
            module = mailbox,
            cases = [
                deliver_then_consume_once,
                peek_does_not_consume,
                per_key_delivery_order,
                keys_are_isolated,
                first_write_wins,
                idempotency_survives_consume,
                deliveries_without_key_all_stored,
                ttl_hides_and_evicts,
            ],
            $($args)*
        );
    };
}

#[cfg(test)]
mod in_memory_queue {
    crate::lease_queue_conformance!(
        runner = futures::executor::block_on,
        harness = crate::contracts::conformance::InMemoryQueueHarness::new(),
    );
}

#[cfg(test)]
mod in_memory_mailbox {
    crate::result_mailbox_conformance!(
        runner = futures::executor::block_on,
        harness = crate::contracts::conformance::InMemoryMailboxHarness::new(),
    );
}

#[cfg(test)]
mod run_all {
    use super::{InMemoryMailboxHarness, InMemoryQueueHarness, mailbox, queue};

    #[test]
    fn run_all_passes_for_in_memory_backends() {
        futures::executor::block_on(async {
            queue::run_all(&InMemoryQueueHarness::new()).await;
            mailbox::run_all(&InMemoryMailboxHarness::new()).await;
        });
    }
}
