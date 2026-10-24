//! The exported store: a durable queue and a result mailbox in one file.

use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    LeaseQueue, LeaseToken, MailboxKey, QueueEntryId, ResultMailbox, ScheduledTask, TaskMetadata,
    now_ms,
};
use prometheus_parking_lot_embedded::{EmbeddedStore, RedbLeaseQueue, RedbResultMailbox};

use crate::error::FfiError;
use crate::payload::Payload;
use crate::runtime::run;
use crate::types::{
    DeliveryResult, DepthInfo, EnqueueOutcome, LeaseHandle, LeasedItem, MailboxAddress,
    NackDisposition, NewTask, ResultDelivery, ResultEntry, StoreConfig, amounts_from, cost_from,
};

struct Inner {
    queue: RedbLeaseQueue<Payload>,
    mailbox: RedbResultMailbox<Payload>,
}

/// An open store. Methods are `async`; from Rust await them on any executor (they
/// run on this crate's own runtime), from Kotlin call them from a coroutine, from
/// Swift with `await`.
///
/// Only one `ParkingLotStore` per file can be open at a time (redb locks the file),
/// within a process and across processes. Release the object (Swift: drop the last
/// reference; Kotlin: `close()`) before reopening the file.
#[derive(uniffi::Object)]
pub struct ParkingLotStore {
    inner: Arc<Inner>,
}

/// Open (or create) the store described by `config`.
///
/// Reopening an existing file restores everything in it: ready, delayed and dead-lettered
/// tasks, leased tasks (which become ready again when their lease expires) and
/// uncollected results.
///
/// # Errors
///
/// `Backend` when the file cannot be created or opened (another process holds it,
/// it is not a store file); `InvalidConfig` when it was written by an incompatible
/// version.
#[uniffi::export]
pub async fn open_store(config: StoreConfig) -> Result<Arc<ParkingLotStore>, FfiError> {
    run(async move {
        let max_depth = usize::try_from(config.max_depth).unwrap_or(usize::MAX);
        let path = config.path;
        // Opening fsyncs and takes a file lock: keep it off the runtime workers.
        let store = tokio::task::spawn_blocking(move || EmbeddedStore::open(path))
            .await
            .map_err(|e| FfiError::Internal {
                detail: format!("store open task failed: {e}"),
            })??;
        Ok(Arc::new(ParkingLotStore {
            inner: Arc::new(Inner {
                queue: store.lease_queue::<Payload>(max_depth),
                mailbox: store.result_mailbox::<Payload>(),
            }),
        }))
    })
    .await
}

fn scheduled(task: NewTask) -> ScheduledTask<Payload> {
    ScheduledTask {
        meta: TaskMetadata {
            id: task.task_id,
            mailbox: task.mailbox.map(MailboxKey::from),
            priority: task.priority.into(),
            cost: cost_from(&task.cost),
            deadline_ms: task.deadline_ms.map(u128::from),
            created_at_ms: now_ms(),
            idempotency_key: task.idempotency_key,
            sequence: 0,
        },
        payload: Payload(task.payload),
    }
}

fn leased(item: prometheus_parking_lot_core::LeasedTask<Payload>) -> LeasedItem {
    let meta = item.task.meta;
    LeasedItem {
        lease: LeaseHandle {
            entry_id: item.token.entry().0,
            fence: item.token.fence(),
        },
        attempt: item.attempt,
        task_id: meta.id,
        payload: item.task.payload.0,
        priority: meta.priority.into(),
        cost: amounts_from(&meta.cost),
        idempotency_key: meta.idempotency_key,
        mailbox: meta.mailbox.map(MailboxAddress::from),
        deadline_ms: meta
            .deadline_ms
            .map(|ms| u64::try_from(ms).unwrap_or(u64::MAX)),
        created_at_ms: u64::try_from(meta.created_at_ms).unwrap_or(u64::MAX),
    }
}

const fn token(handle: LeaseHandle) -> LeaseToken {
    LeaseToken::new(QueueEntryId(handle.entry_id), handle.fence)
}

#[uniffi::export]
impl ParkingLotStore {
    /// Store `task` durably (fsynced before this returns).
    ///
    /// # Errors
    ///
    /// `QueueFull` at `max_depth` live entries; `Backend` on a storage failure.
    pub async fn enqueue(&self, task: NewTask) -> Result<EnqueueOutcome, FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move {
            let outcome = inner.queue.enqueue(scheduled(task)).await?;
            Ok(EnqueueOutcome {
                entry_id: outcome.entry().0,
                duplicate: matches!(outcome, prometheus_parking_lot_core::Enqueued::Duplicate(_)),
            })
        })
        .await
    }

    /// Take up to `max` ready tasks, highest priority first, each invisible to other
    /// consumers for `lease_ms`. Ack a task when its work is done; if the lease
    /// expires first the task is handed out again with a higher `attempt`.
    ///
    /// # Errors
    ///
    /// `InvalidConfig` for a zero `lease_ms`; `Backend` on a storage failure.
    pub async fn lease(&self, max: u32, lease_ms: u64) -> Result<Vec<LeasedItem>, FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move {
            let items = inner
                .queue
                .lease(
                    usize::try_from(max).unwrap_or(usize::MAX),
                    Duration::from_millis(lease_ms),
                )
                .await?;
            Ok(items.into_iter().map(leased).collect())
        })
        .await
    }

    /// Mark a leased task done and remove it from the queue.
    ///
    /// # Errors
    ///
    /// `LeaseLost` when the lease expired or was already settled.
    pub async fn ack(&self, lease: LeaseHandle) -> Result<(), FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move { Ok(inner.queue.ack(token(lease)).await?) }).await
    }

    /// Give a leased task back: requeue it (optionally after a delay) or dead-letter it.
    ///
    /// # Errors
    ///
    /// `LeaseLost` when the lease expired or was already settled.
    pub async fn nack(
        &self,
        lease: LeaseHandle,
        disposition: NackDisposition,
    ) -> Result<(), FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move { Ok(inner.queue.nack(token(lease), disposition.into()).await?) }).await
    }

    /// Hold a lease for `lease_ms` more from now (a heartbeat for long-running work).
    ///
    /// # Errors
    ///
    /// `LeaseLost` when the lease already expired; `InvalidConfig` for zero.
    pub async fn extend_lease(&self, lease: LeaseHandle, lease_ms: u64) -> Result<(), FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move {
            Ok(inner
                .queue
                .extend(token(lease), Duration::from_millis(lease_ms))
                .await?)
        })
        .await
    }

    /// Entries per state.
    ///
    /// # Errors
    ///
    /// `Backend` on a storage failure.
    pub async fn depth(&self) -> Result<DepthInfo, FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move { Ok(inner.queue.depth().await?.into()) }).await
    }

    /// Store a task's outcome in its mailbox. Deliver the outcome before acking the
    /// task: a crash in between then reruns the task, and the idempotency key keeps
    /// the rerun from storing a second result.
    ///
    /// # Errors
    ///
    /// `Backend` on a storage failure.
    pub async fn deliver_result(
        &self,
        delivery: ResultDelivery,
    ) -> Result<DeliveryResult, FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move {
            let outcome = inner.mailbox.deliver(delivery.into_delivery()).await?;
            Ok(match outcome {
                prometheus_parking_lot_core::DeliveryOutcome::Stored => DeliveryResult::Stored,
                prometheus_parking_lot_core::DeliveryOutcome::Duplicate { .. } => {
                    DeliveryResult::Duplicate
                }
            })
        })
        .await
    }

    /// Take the oldest uncollected result for `mailbox`; no later call returns it
    /// again. `None` when there is none (or it expired).
    ///
    /// # Errors
    ///
    /// `Backend` on a storage failure.
    pub async fn consume_result(
        &self,
        mailbox: MailboxAddress,
    ) -> Result<Option<ResultEntry>, FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move {
            let key = MailboxKey::from(mailbox);
            Ok(inner.mailbox.consume(&key).await?.map(ResultEntry::from))
        })
        .await
    }

    /// Read the oldest uncollected result for `mailbox` without removing it.
    ///
    /// # Errors
    ///
    /// `Backend` on a storage failure.
    pub async fn peek_result(
        &self,
        mailbox: MailboxAddress,
    ) -> Result<Option<ResultEntry>, FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move {
            let key = MailboxKey::from(mailbox);
            Ok(inner.mailbox.peek(&key).await?.map(ResultEntry::from))
        })
        .await
    }

    /// Remove results whose TTL has passed; returns how many.
    ///
    /// # Errors
    ///
    /// `Backend` on a storage failure.
    pub async fn evict_expired_results(&self) -> Result<u64, FfiError> {
        let inner = Arc::clone(&self.inner);
        run(async move { Ok(crate::error::widen(inner.mailbox.evict_expired().await?)) }).await
    }
}
