//! Negative controls: deliberately broken backends the suite must reject, so a
//! passing run means the cases can fail.

use std::collections::HashMap;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::time::Duration;

use parking_lot::Mutex;

use super::{
    InMemoryMailboxHarness, InMemoryQueueHarness, MailboxHarness, QueueHarness, mailbox, queue,
};
use crate::contracts::error::SchedulerError;
use crate::contracts::lease_queue::{
    Enqueued, LeaseQueue, LeaseToken, LeasedTask, NackAction, QueueDepth,
};
use crate::contracts::result_mailbox::{Delivery, DeliveryOutcome, MailboxEntry, ResultMailbox};
use crate::contracts::task::{ScheduledTask, TaskStatus};
use crate::contracts::types::MailboxKey;
use crate::contracts::{InMemoryQueue, TaskQueue};

/// A queue that accepts every ack, even with a stale token (no fencing).
struct UnfencedAck<Q>(Q);

impl<Q: LeaseQueue<String>> LeaseQueue<String> for UnfencedAck<Q> {
    fn enqueue(
        &self,
        task: ScheduledTask<String>,
    ) -> impl Future<Output = Result<Enqueued, SchedulerError>> + Send {
        self.0.enqueue(task)
    }

    fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> impl Future<Output = Result<Vec<LeasedTask<String>>, SchedulerError>> + Send {
        self.0.lease(max, lease_for)
    }

    fn ack(&self, _token: LeaseToken) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        std::future::ready(Ok(()))
    }

    fn nack(
        &self,
        token: LeaseToken,
        action: NackAction,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.0.nack(token, action)
    }

    fn extend(
        &self,
        token: LeaseToken,
        lease_for: Duration,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.0.extend(token, lease_for)
    }

    fn depth(&self) -> impl Future<Output = Result<QueueDepth, SchedulerError>> + Send {
        self.0.depth()
    }

    fn max_depth(&self) -> usize {
        self.0.max_depth()
    }

    fn identity(&self) -> &str {
        self.0.identity()
    }
}

struct UnfencedHarness(InMemoryQueueHarness);

impl QueueHarness for UnfencedHarness {
    type Queue = UnfencedAck<<InMemoryQueueHarness as QueueHarness>::Queue>;

    async fn queue(&self, max_depth: usize) -> Self::Queue {
        UnfencedAck(self.0.queue(max_depth).await)
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.0.advance(by)
    }
}

/// A mailbox that ignores idempotency keys (no first-write-wins).
struct NoDedup<M>(M);

impl<M: ResultMailbox<String>> ResultMailbox<String> for NoDedup<M> {
    fn deliver(
        &self,
        mut delivery: Delivery<String>,
    ) -> impl Future<Output = Result<DeliveryOutcome, SchedulerError>> + Send {
        delivery.idempotency_key = None;
        self.0.deliver(delivery)
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.0.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.0.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.0.evict_expired()
    }
}

struct NoDedupHarness(InMemoryMailboxHarness);

impl MailboxHarness for NoDedupHarness {
    type Mailbox = NoDedup<<InMemoryMailboxHarness as MailboxHarness>::Mailbox>;

    async fn mailbox(&self) -> Self::Mailbox {
        NoDedup(self.0.mailbox().await)
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.0.advance(by)
    }
}

/// The pre-0.4 defect: a remembered caller key suppresses every address.
struct MailboxGlobalKeys<M> {
    inner: M,
    remembered: Mutex<HashMap<String, TaskStatus>>,
}

impl<M: ResultMailbox<String>> ResultMailbox<String> for MailboxGlobalKeys<M> {
    async fn deliver(&self, delivery: Delivery<String>) -> Result<DeliveryOutcome, SchedulerError> {
        let original = delivery.idempotency_key.as_ref().and_then(|key| {
            let mut remembered = self.remembered.lock();
            let original = remembered.get(key).cloned();
            if original.is_none() {
                remembered.insert(key.clone(), delivery.status.clone());
            }
            original
        });
        if let Some(status) = original {
            return Ok(DeliveryOutcome::Duplicate { status });
        }
        self.inner.deliver(delivery).await
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.inner.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.inner.evict_expired()
    }
}

struct MailboxGlobalKeysHarness(InMemoryMailboxHarness);

impl MailboxHarness for MailboxGlobalKeysHarness {
    type Mailbox = MailboxGlobalKeys<<InMemoryMailboxHarness as MailboxHarness>::Mailbox>;

    async fn mailbox(&self) -> Self::Mailbox {
        MailboxGlobalKeys {
            inner: self.0.mailbox().await,
            remembered: Mutex::new(HashMap::new()),
        }
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.0.advance(by)
    }
}

fn fails(case: impl Future<Output = ()>) -> bool {
    catch_unwind(AssertUnwindSafe(|| futures::executor::block_on(case))).is_err()
}

#[test]
fn suite_rejects_a_queue_without_fencing() {
    let harness = UnfencedHarness(InMemoryQueueHarness::new());
    assert!(fails(queue::lease_expiry_redelivers(&harness)));
    assert!(fails(queue::ack_removes_task(&harness)));
}

#[test]
fn suite_rejects_a_mailbox_without_first_write_wins() {
    let harness = NoDedupHarness(InMemoryMailboxHarness::new());
    assert!(fails(mailbox::first_write_wins(&harness)));
    assert!(fails(mailbox::idempotency_survives_consume(&harness)));
}

#[test]
fn suite_rejects_mailbox_global_idempotency_keys() {
    let harness = MailboxGlobalKeysHarness(InMemoryMailboxHarness::new());
    assert!(fails(mailbox::idempotency_is_scoped_to_mailbox_key(
        &harness
    )));
}

/// The old bool-only interpretation: replace original status with retry status.
struct LocalDuplicateStatus<M>(M);

impl<M: ResultMailbox<String>> ResultMailbox<String> for LocalDuplicateStatus<M> {
    async fn deliver(&self, delivery: Delivery<String>) -> Result<DeliveryOutcome, SchedulerError> {
        let retry_status = delivery.status.clone();
        match self.0.deliver(delivery).await? {
            DeliveryOutcome::Stored => Ok(DeliveryOutcome::Stored),
            DeliveryOutcome::Duplicate { .. } => Ok(DeliveryOutcome::Duplicate {
                status: retry_status,
            }),
        }
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.0.consume(key)
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<String>>, SchedulerError>> + Send {
        self.0.peek(key)
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.0.evict_expired()
    }
}

struct LocalDuplicateStatusHarness(InMemoryMailboxHarness);

impl MailboxHarness for LocalDuplicateStatusHarness {
    type Mailbox = LocalDuplicateStatus<<InMemoryMailboxHarness as MailboxHarness>::Mailbox>;

    async fn mailbox(&self) -> Self::Mailbox {
        LocalDuplicateStatus(self.0.mailbox().await)
    }

    fn advance(&self, by: Duration) -> impl Future<Output = ()> + Send {
        self.0.advance(by)
    }
}

#[test]
fn suite_rejects_duplicate_status_from_the_retry() {
    let harness = LocalDuplicateStatusHarness(InMemoryMailboxHarness::new());
    assert!(fails(mailbox::duplicate_returns_original_status(&harness)));
    assert!(fails(
        mailbox::duplicate_status_expires_with_original_fence(&harness)
    ));
}

/// A count-only legacy adapter removes real tasks but discards their metadata.
struct CountOnlyPrune<Q>(Q);

impl<Q: TaskQueue<String>> TaskQueue<String> for CountOnlyPrune<Q> {
    fn enqueue(&mut self, task: ScheduledTask<String>) -> Result<(), SchedulerError> {
        self.0.enqueue(task)
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<String>>, SchedulerError> {
        self.0.dequeue()
    }

    fn prune_expired(
        &mut self,
        now_ms: u128,
    ) -> Result<Vec<ScheduledTask<String>>, SchedulerError> {
        self.0.prune_expired(now_ms).map(|_| Vec::new())
    }

    fn max_depth(&self) -> usize {
        self.0.max_depth()
    }

    fn len(&self) -> usize {
        self.0.len()
    }
}

#[test]
fn suite_rejects_pruning_without_removed_metadata() {
    let mut queue = CountOnlyPrune(InMemoryQueue::new(8));
    assert!(fails(async {
        super::task_queue::prune_returns_all_removed_tasks(&mut queue);
    }));
}
