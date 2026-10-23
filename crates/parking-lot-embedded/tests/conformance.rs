//! The shared backend conformance suite (ADR 0003) against the redb queue and
//! mailbox: every lease, ordering, idempotency and TTL rule, on real files.

use std::future::Future;
use std::sync::Mutex;
use std::time::Duration;

use prometheus_parking_lot_core::conformance::{MailboxHarness, QueueHarness};
use prometheus_parking_lot_core::{TestClock, lease_queue_conformance, result_mailbox_conformance};
use prometheus_parking_lot_embedded::{EmbeddedStore, RedbLeaseQueue, RedbResultMailbox};
use tempfile::TempDir;

/// Wall time every harness clock starts at (arbitrary, non-zero).
const START_MS: u128 = 1_700_000_000_000;

/// Builds a fresh database file per queue; time moves only through `advance`.
struct RedbQueueHarness {
    clock: TestClock,
    dirs: Mutex<Vec<TempDir>>,
}

impl RedbQueueHarness {
    fn new() -> Self {
        Self {
            clock: TestClock::new(START_MS),
            dirs: Mutex::default(),
        }
    }
}

impl QueueHarness for RedbQueueHarness {
    type Queue = RedbLeaseQueue<String>;

    fn queue(&self, max_depth: usize) -> impl Future<Output = Self::Queue> + Send {
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            EmbeddedStore::open_with_clock(dir.path().join("queue.redb"), self.clock.clone())
                .expect("open store");
        self.dirs.lock().expect("dirs").push(dir);
        std::future::ready(store.lease_queue(max_depth))
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.clock.advance(by);
        std::future::ready(())
    }
}

/// Builds a fresh database file per mailbox.
struct RedbMailboxHarness {
    clock: TestClock,
    dirs: Mutex<Vec<TempDir>>,
}

impl RedbMailboxHarness {
    fn new() -> Self {
        Self {
            clock: TestClock::new(START_MS),
            dirs: Mutex::default(),
        }
    }
}

impl MailboxHarness for RedbMailboxHarness {
    type Mailbox = RedbResultMailbox<String>;

    fn mailbox(&self) -> impl Future<Output = Self::Mailbox> + Send {
        let dir = tempfile::tempdir().expect("tempdir");
        let store =
            EmbeddedStore::open_with_clock(dir.path().join("mailbox.redb"), self.clock.clone())
                .expect("open store");
        self.dirs.lock().expect("dirs").push(dir);
        std::future::ready(store.result_mailbox())
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.clock.advance(by);
        std::future::ready(())
    }
}

mod lease_queue {
    use super::*;

    lease_queue_conformance!(
        test_attr = #[tokio::test],
        harness = RedbQueueHarness::new(),
    );
}

mod result_mailbox {
    use super::*;

    result_mailbox_conformance!(
        test_attr = #[tokio::test],
        harness = RedbMailboxHarness::new(),
    );
}
