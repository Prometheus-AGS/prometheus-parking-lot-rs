//! [`IggyNotifier`]: `mailbox.ready` for each stored result.

use std::future::Future;

use prometheus_parking_lot_core::{MailboxEntry, MailboxKey, Notifier, SchedulerError};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::runtime::Handle;

use crate::ids::{instance_nonce, mailbox_ready_id};
use crate::options::IggyOptions;
use crate::publisher::{Envelope, Publisher, PublisherStats};

/// Publishes `mailbox.ready` when a result has been stored.
///
/// The message is a pointer, not the result: the mailbox key, task id, status,
/// idempotency key and a `result_ref` to read the entry from the mailbox
/// storage. Its id is the `UUIDv5` of the idempotency key, so a re-notification
/// of the same result carries the same id. A result without an idempotency key
/// gets a distinct id per notification (task ids are caller-supplied and can
/// repeat). Partitioned by tenant.
///
/// `notify` only queues the message. Used through a
/// [`BackgroundNotifier`](prometheus_parking_lot_core::mailbox::BackgroundNotifier)
/// a full queue is reported as an error and retried with backoff, then
/// abandoned and counted; the stored result is never affected.
#[derive(Debug, Clone)]
pub struct IggyNotifier {
    publisher: Arc<Publisher>,
    nonce: u128,
    sequence: Arc<AtomicU64>,
}

impl IggyNotifier {
    /// Start a notifier publishing on the Tokio runtime the caller is in.
    ///
    /// # Errors
    ///
    /// Outside a Tokio runtime.
    pub fn start(options: &IggyOptions) -> Result<Self, tokio::runtime::TryCurrentError> {
        Ok(Self::start_on(&Handle::try_current()?, options))
    }

    /// Start a notifier publishing on `handle`'s runtime.
    #[must_use]
    pub fn start_on(handle: &Handle, options: &IggyOptions) -> Self {
        Self {
            publisher: Arc::new(Publisher::start(handle, options, &options.mailbox_topic)),
            nonce: instance_nonce(),
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The publisher's counters.
    #[must_use]
    pub fn stats(&self) -> PublisherStats {
        self.publisher.stats()
    }

    /// Wait until the queued notifications are published; see
    /// [`Publisher::flush`].
    pub async fn flush(&self, timeout: std::time::Duration) -> bool {
        self.publisher.flush(timeout).await
    }
}

/// Where the entry can be read: `tenant[/user][/session]`.
fn result_ref(key: &MailboxKey) -> String {
    let mut reference = key.tenant.clone();
    for part in [&key.user_id, &key.session_id].into_iter().flatten() {
        reference.push('/');
        reference.push_str(part);
    }
    reference
}

impl<T: Sync> Notifier<T> for IggyNotifier {
    fn notify(
        &self,
        key: &MailboxKey,
        entry: &MailboxEntry<T>,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        let reference = result_ref(key);
        // Without an idempotency key nothing identifies the result: task ids
        // are caller-supplied and can repeat. Each notification then gets its
        // own id (this notifier's nonce and sequence), so none is mistaken for
        // a repeat; set an idempotency key to have repeats recognized.
        let stable_key = entry.idempotency_key.clone().unwrap_or_else(|| {
            let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
            format!(
                "task:{}:{reference}:{:?}:{}:{:x}:{sequence}",
                entry.task_id, entry.status, entry.delivered_at_ms, self.nonce
            )
        });
        let body = json!({
            "event": "mailbox.ready",
            "mailbox_key": {
                "tenant": key.tenant,
                "user_id": key.user_id,
                "session_id": key.session_id,
            },
            "task_id": entry.task_id,
            "status": entry.status,
            "idempotency_key": entry.idempotency_key,
            "result_ref": reference,
            "delivered_at_ms": entry.delivered_at_ms.to_string(),
        });
        let envelope = Envelope::new(
            mailbox_ready_id(&stable_key),
            key.tenant.clone(),
            serde_json::to_vec(&body).unwrap_or_default(),
        );
        let queued = self.publisher.enqueue(envelope).map_err(|_| {
            SchedulerError::backend(
                "iggy notifier",
                std::io::Error::other(
                    "publisher queue is full or stopped; notification not queued",
                ),
            )
        });
        std::future::ready(queued)
    }
}
