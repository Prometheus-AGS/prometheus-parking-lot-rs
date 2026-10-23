//! The Tauri example's queue logic, kept free of any GUI so it can be tested without
//! a window: submit text, let a worker process it, collect the results.
//!
//! The shape is the one a real desktop app has. A task is durable the moment `submit`
//! returns, so closing the app (or crashing it) between submit and processing loses
//! nothing; a result is durable the moment `process_next` stores it, so it waits in
//! the mailbox for the window to ask, even if the window was closed when the work
//! finished. `process_next` stores the result before acking the task, the order that
//! makes a crash in between harmless: the task runs again and its idempotency key
//! stops a second result being stored.

use std::time::{SystemTime, UNIX_EPOCH};

use parking_lot_ffi::{
    FfiError, MailboxAddress, NackDisposition, NewTask, ParkingLotStore, ResourceAmount,
    ResourceClass, ResultDelivery, StoreConfig, TaskOutcome, TaskPriority, open_store,
};
use serde::Serialize;
use std::sync::Arc;

/// How long a worker may hold a task before it counts as dead and the task is handed
/// out again.
const LEASE_MS: u64 = 30_000;
/// Live tasks the example's queue accepts.
const MAX_DEPTH: u64 = 1_000;

/// One task's result as the window shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Collected {
    /// The id `submit` returned.
    pub task_id: u64,
    /// The processed text.
    pub text: String,
}

/// What `process_next` did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Processed {
    /// The id `submit` returned.
    pub task_id: u64,
    /// 1 for a first run; more means the task was redelivered after a lapsed lease.
    pub attempt: u32,
}

/// Queue state for the status line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Status {
    /// Tasks waiting to be processed.
    pub waiting: u64,
    /// Tasks held by a worker right now.
    pub in_progress: u64,
}

/// The example's queue: one store file, one mailbox.
#[derive(Clone)]
pub struct AppQueue {
    store: Arc<ParkingLotStore>,
}

fn mailbox() -> MailboxAddress {
    MailboxAddress {
        tenant: "tauri-example".to_owned(),
        user_id: None,
        session_id: None,
    }
}

/// A task id that is unique across restarts: microseconds since the epoch.
fn new_task_id() -> u64 {
    let micros = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_micros());
    u64::try_from(micros).unwrap_or(u64::MAX)
}

/// The "work": stands in for an inference call or any job worth queueing.
fn work(text: &str) -> String {
    text.to_uppercase()
}

impl AppQueue {
    /// Open (or create) the store file at `path`. Everything the previous run left in
    /// the file, queued tasks and uncollected results, is there.
    ///
    /// # Errors
    ///
    /// The store cannot be opened (the file is held by another process, or is not a
    /// store file).
    pub async fn open(path: impl Into<String>) -> Result<Self, FfiError> {
        let store = open_store(StoreConfig {
            path: path.into(),
            max_depth: MAX_DEPTH,
        })
        .await?;
        Ok(Self { store })
    }

    /// Queue `text` for processing and return its task id. Durable on return.
    ///
    /// # Errors
    ///
    /// The queue is full, or the disk write failed.
    pub async fn submit(&self, text: &str) -> Result<u64, FfiError> {
        let task_id = new_task_id();
        let _ = self
            .store
            .enqueue(NewTask {
                task_id,
                payload: text.as_bytes().to_vec(),
                priority: TaskPriority::Normal,
                cost: vec![ResourceAmount {
                    resource: ResourceClass::Cpu,
                    units: 1,
                }],
                idempotency_key: Some(format!("task-{task_id}")),
                mailbox: Some(mailbox()),
                deadline_ms: None,
            })
            .await?;
        Ok(task_id)
    }

    /// Process the next waiting task, if any: lease it, do the work, store the
    /// result, then ack. A task whose payload is not UTF-8 is dead-lettered rather
    /// than retried forever.
    ///
    /// # Errors
    ///
    /// A storage failure; the task stays queued and is retried after its lease lapses.
    pub async fn process_next(&self) -> Result<Option<Processed>, FfiError> {
        let Some(item) = self.store.lease(1, LEASE_MS).await?.into_iter().next() else {
            return Ok(None);
        };
        let Ok(text) = String::from_utf8(item.payload.clone()) else {
            self.store
                .nack(
                    item.lease,
                    NackDisposition::DeadLetter {
                        reason: "payload is not UTF-8 text".to_owned(),
                    },
                )
                .await?;
            return Ok(None);
        };
        let _ = self
            .store
            .deliver_result(ResultDelivery {
                mailbox: item.mailbox.clone().unwrap_or_else(mailbox),
                task_id: item.task_id,
                outcome: TaskOutcome::Completed,
                payload: Some(work(&text).into_bytes()),
                idempotency_key: item.idempotency_key.clone(),
                ttl_ms: None,
            })
            .await?;
        self.store.ack(item.lease).await?;
        Ok(Some(Processed {
            task_id: item.task_id,
            attempt: item.attempt,
        }))
    }

    /// Take every result waiting in the mailbox, oldest first.
    ///
    /// # Errors
    ///
    /// A storage failure.
    pub async fn collect(&self) -> Result<Vec<Collected>, FfiError> {
        let mut results = Vec::new();
        while let Some(entry) = self.store.consume_result(mailbox()).await? {
            results.push(Collected {
                task_id: entry.task_id,
                text: String::from_utf8_lossy(&entry.payload.unwrap_or_default()).into_owned(),
            });
        }
        Ok(results)
    }

    /// Queue state for the status line.
    ///
    /// # Errors
    ///
    /// A storage failure.
    pub async fn status(&self) -> Result<Status, FfiError> {
        let depth = self.store.depth().await?;
        Ok(Status {
            waiting: depth.ready + depth.delayed,
            in_progress: depth.leased,
        })
    }
}
