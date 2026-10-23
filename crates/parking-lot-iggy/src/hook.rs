//! [`IggyHook`]: lifecycle events to the `lifecycle` topic.

use prometheus_parking_lot_core::{Hook, LifecycleEvent};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::runtime::Handle;

use crate::ids::{instance_nonce, message_id};
use crate::options::IggyOptions;
use crate::publisher::{Envelope, Publisher, PublisherStats};

/// Publishes every [`LifecycleEvent`] to the `lifecycle` topic.
///
/// `on_event` serializes the event and queues it; the network is touched only
/// by the publisher's own task. Messages are partitioned by the configured
/// tenant, or by pool name. Every event gets its own message id (a `UUIDv5` of
/// this hook's instance nonce and the event's sequence number), because task
/// ids and idempotency keys are caller-supplied and can repeat; a publisher
/// retry resends the same message with the same id. The hook does not apply the
/// publisher's repeat-skipping window to events.
#[derive(Debug, Clone)]
pub struct IggyHook {
    publisher: Arc<Publisher>,
    tenant: Option<String>,
    /// Makes the ids of this hook's messages differ from every other hook's,
    /// including one in an earlier run of the process.
    nonce: u128,
    /// Numbers the events of this hook.
    sequence: Arc<AtomicU64>,
}

impl IggyHook {
    /// Start a hook publishing on the Tokio runtime the caller is in.
    ///
    /// # Errors
    ///
    /// Outside a Tokio runtime.
    pub fn start(options: &IggyOptions) -> Result<Self, tokio::runtime::TryCurrentError> {
        Ok(Self::start_on(&Handle::try_current()?, options))
    }

    /// Start a hook publishing on `handle`'s runtime.
    #[must_use]
    pub fn start_on(handle: &Handle, options: &IggyOptions) -> Self {
        Self {
            publisher: Arc::new(Publisher::start(handle, options, &options.lifecycle_topic)),
            tenant: options.tenant.clone(),
            nonce: instance_nonce(),
            sequence: Arc::new(AtomicU64::new(0)),
        }
    }

    /// The publisher's counters.
    #[must_use]
    pub fn stats(&self) -> PublisherStats {
        self.publisher.stats()
    }

    /// Wait until the queued events are published; see [`Publisher::flush`].
    pub async fn flush(&self, timeout: std::time::Duration) -> bool {
        self.publisher.flush(timeout).await
    }
}

impl Hook for IggyHook {
    fn on_event(&self, event: &LifecycleEvent) {
        let (stable_key, body) = describe(event);
        // Task ids and idempotency keys are caller-supplied and may repeat, and
        // two events of one task can share a millisecond, so nothing in the
        // event identifies it. The id is therefore per published event (this
        // hook's nonce and sequence): a publisher retry resends the same
        // envelope, so it keeps the same id, and a consumer de-duplicates on it.
        let sequence = self.sequence.fetch_add(1, Ordering::Relaxed);
        let id = message_id(
            "lifecycle",
            &format!(
                "{:x}\u{1f}{sequence}\u{1f}{}\u{1f}{}\u{1f}{stable_key}",
                self.nonce,
                event.pool(),
                event.kind()
            ),
        );
        let key = self
            .tenant
            .clone()
            .unwrap_or_else(|| event.pool().to_owned());
        let payload = serde_json::to_vec(&body).unwrap_or_default();
        // A full queue is counted by the publisher; nothing else to do here.
        let _ = self
            .publisher
            .enqueue(Envelope::new(id, key, payload).without_dedup());
    }
}

/// The event's stable key (idempotency key or task id, plus attempt and time)
/// and its JSON body.
fn describe(event: &LifecycleEvent) -> (String, Value) {
    let mut body = json!({
        "event": event.kind(),
        "pool": event.pool(),
        "task_id": event.task_id(),
        "terminal": event.is_terminal(),
    });
    let (idempotency_key, attempt, at_ms) = match event {
        LifecycleEvent::Submitted {
            idempotency_key,
            at_ms,
            ..
        } => (idempotency_key.as_deref(), 0, *at_ms),
        LifecycleEvent::Cancelled {
            idempotency_key,
            attempt,
            at_ms,
            reason,
            ..
        } => {
            body["reason"] = json!(format!("{reason:?}"));
            (idempotency_key.as_deref(), *attempt, *at_ms)
        }
        LifecycleEvent::Requeued {
            idempotency_key,
            attempt,
            at_ms,
            ..
        } => (idempotency_key.as_deref(), *attempt, *at_ms),
        LifecycleEvent::Queued {
            queue_depth, at_ms, ..
        } => {
            body["queue_depth"] = json!(queue_depth);
            (None, 0, *at_ms)
        }
        LifecycleEvent::Started {
            worker_id, at_ms, ..
        } => {
            body["worker_id"] = json!(worker_id);
            (None, 0, *at_ms)
        }
        LifecycleEvent::Completed {
            duration_ms, at_ms, ..
        } => {
            body["duration_ms"] = json!(duration_ms);
            (None, 0, *at_ms)
        }
        LifecycleEvent::Failed { error, at_ms, .. } => {
            body["error"] = json!(error);
            (None, 0, *at_ms)
        }
        LifecycleEvent::Rejected { reason, at_ms, .. } => {
            body["reason"] = json!(format!("{reason:?}"));
            (None, 0, *at_ms)
        }
        LifecycleEvent::Evicted { at_ms, .. } | LifecycleEvent::Expired { at_ms, .. } => {
            (None, 0, *at_ms)
        }
        _ => (None, 0, 0),
    };
    body["idempotency_key"] = json!(idempotency_key);
    body["attempt"] = json!(attempt);
    body["at_ms"] = json!(at_ms.to_string());
    let task = idempotency_key.map_or_else(|| event.task_id().to_string(), str::to_owned);
    (format!("{task}\u{1f}{attempt}\u{1f}{at_ms}"), body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describe_carries_the_idempotency_key_and_attempt() {
        let event = LifecycleEvent::Requeued {
            task_id: 7,
            idempotency_key: Some("job-7".to_owned()),
            pool: "gpu".to_owned(),
            attempt: 2,
            at_ms: 11,
        };
        let (stable, body) = describe(&event);
        assert_eq!(stable, "job-7\u{1f}2\u{1f}11");
        assert_eq!(body["event"], "requeued");
        assert_eq!(body["idempotency_key"], "job-7");
        assert_eq!(body["terminal"], true);
        assert_eq!(body["at_ms"], "11");
    }

    #[test]
    fn describe_falls_back_to_the_task_id() {
        let event = LifecycleEvent::Completed {
            task_id: 9,
            pool: "gpu".to_owned(),
            duration_ms: 3,
            at_ms: 5,
        };
        let (stable, body) = describe(&event);
        assert_eq!(stable, "9\u{1f}0\u{1f}5");
        assert_eq!(body["duration_ms"], 3);
        assert!(body["idempotency_key"].is_null());
    }
}
