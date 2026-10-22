//! In-memory mailbox backend with TTL/expiry and consume-once semantics.
//!
//! ## Semantics
//!
//! - **Delivery:** at-least-once for persistent backends; in-memory provides
//!   no cross-restart durability (documented limitation).
//! - **Idempotency (D5):** when an `idempotency_key` is present delivery is
//!   first-write-wins; a second `deliver` for the same key is silently dropped
//!   to prevent duplicate results from at-least-once redeliveries.
//! - **Consume-once:** `consume` takes and removes the message; subsequent
//!   calls for the same key return `None`.
//! - **TTL/expiry:** `deliver_with_ttl` stores the deadline; `evict_expired`
//!   removes stale messages.

use std::collections::HashMap;

use crate::core::{Mailbox, SchedulerError, TaskStatus};
use crate::util::serde::MailboxKey;

/// A single mailbox message with optional TTL.
#[derive(Debug, Clone)]
pub struct MailboxMessage<P> {
    /// Task status.
    pub status: TaskStatus,
    /// Optional payload/result.
    pub payload: Option<P>,
    /// Timestamp milliseconds when the message was delivered.
    pub created_at_ms: u128,
    /// Absolute expiry in milliseconds since epoch, if set.
    pub expires_at_ms: Option<u128>,
    /// Idempotency key, if the originating task had one set.
    pub idempotency_key: Option<String>,
}

impl<P> MailboxMessage<P> {
    fn is_expired(&self, now_ms: u128) -> bool {
        self.expires_at_ms.map(|exp| now_ms >= exp).unwrap_or(false)
    }
}

/// In-memory mailbox for development and testing.
///
/// **Limitation:** no cross-restart durability. Use a persistent backend
/// (`PostgresMailbox`, `YaqueMailbox`) for production.
pub struct InMemoryMailbox<P> {
    /// Keyed by MailboxKey. At most one message per key (consume-once).
    messages: HashMap<MailboxKey, MailboxMessage<P>>,
    /// Optional secondary index: idempotency_key → MailboxKey (for dedup).
    idem_index: HashMap<String, MailboxKey>,
}

impl<P> Default for InMemoryMailbox<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> InMemoryMailbox<P> {
    /// Create a new mailbox.
    pub fn new() -> Self {
        Self {
            messages: HashMap::new(),
            idem_index: HashMap::new(),
        }
    }

    /// Peek at the message for `key` without consuming it.
    pub fn peek(&self, key: &MailboxKey) -> Option<&MailboxMessage<P>> {
        self.messages.get(key)
    }

    /// Number of messages currently stored.
    pub fn len(&self) -> usize {
        self.messages.len()
    }

    /// True if no messages are stored.
    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    fn deliver_inner(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<P>,
        expires_at_ms: Option<u128>,
        idempotency_key: Option<String>,
    ) -> Result<(), SchedulerError> {
        // D5 — first-write-wins: if an idempotency key is set and we've already
        // delivered a result for it, silently drop the duplicate.
        if let Some(ref idem) = idempotency_key {
            if self.idem_index.contains_key(idem.as_str()) {
                return Ok(());
            }
            self.idem_index.insert(idem.clone(), key.clone());
        }

        // Overwrite any existing entry (e.g. status update from Queued → Completed).
        self.messages.insert(
            key.clone(),
            MailboxMessage {
                status,
                payload,
                created_at_ms: crate::util::clock::now_ms(),
                expires_at_ms,
                idempotency_key,
            },
        );
        Ok(())
    }
}

impl<P: Clone> Mailbox<P> for InMemoryMailbox<P> {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<P>,
    ) -> Result<(), SchedulerError> {
        self.deliver_inner(key, status, payload, None, None)
    }

    fn deliver_with_ttl(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<P>,
        ttl_ms: u64,
    ) -> Result<(), SchedulerError> {
        let expires_at = crate::util::clock::now_ms() + u128::from(ttl_ms);
        self.deliver_inner(key, status, payload, Some(expires_at), None)
    }

    fn consume(
        &mut self,
        key: &MailboxKey,
    ) -> Result<Option<(TaskStatus, Option<P>)>, SchedulerError> {
        if let Some(msg) = self.messages.remove(key) {
            // Clean up idempotency index
            if let Some(idem) = &msg.idempotency_key {
                self.idem_index.remove(idem.as_str());
            }
            Ok(Some((msg.status, msg.payload)))
        } else {
            Ok(None)
        }
    }

    fn evict_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        let before = self.messages.len();
        let mut to_remove: Vec<MailboxKey> = Vec::new();
        for (key, msg) in &self.messages {
            if msg.is_expired(now_ms) {
                to_remove.push(key.clone());
            }
        }
        for key in &to_remove {
            if let Some(msg) = self.messages.remove(key) {
                if let Some(idem) = &msg.idempotency_key {
                    self.idem_index.remove(idem.as_str());
                }
            }
        }
        Ok(before.saturating_sub(self.messages.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::util::serde::MailboxKey;

    fn key(s: &str) -> MailboxKey {
        MailboxKey {
            tenant: s.into(),
            user_id: None,
            session_id: None,
        }
    }

    #[test]
    fn deliver_and_consume_once() {
        let mut mb = InMemoryMailbox::<String>::new();
        let k = key("t1");
        mb.deliver(&k, TaskStatus::Completed, Some("result".into()))
            .unwrap();
        assert_eq!(mb.len(), 1);

        // Consume retrieves and removes
        let result = mb.consume(&k).unwrap();
        assert!(result.is_some());
        assert!(matches!(result.unwrap().0, TaskStatus::Completed));

        // Second consume returns None (consume-once)
        assert!(mb.consume(&k).unwrap().is_none());
        assert_eq!(mb.len(), 0);
    }

    #[test]
    fn first_write_wins_idempotency() {
        let mut mb = InMemoryMailbox::<String>::new();
        let k = key("t1");

        mb.deliver_inner(
            &k,
            TaskStatus::Completed,
            Some("first".into()),
            None,
            Some("idem-1".into()),
        )
        .unwrap();
        // Duplicate with same idempotency key — must be silently dropped
        let k2 = key("t1-dup");
        mb.deliver_inner(
            &k2,
            TaskStatus::Completed,
            Some("second".into()),
            None,
            Some("idem-1".into()),
        )
        .unwrap();

        // Only the first message exists
        assert_eq!(mb.len(), 1);
        let (_, payload) = mb.consume(&k).unwrap().unwrap();
        assert_eq!(payload.unwrap(), "first");
    }

    #[test]
    fn ttl_eviction() {
        let mut mb = InMemoryMailbox::<String>::new();
        let k1 = key("t1");
        let k2 = key("t2");

        // k1: expires at ms=100
        mb.deliver_with_ttl(&k1, TaskStatus::Completed, None, 0)
            .unwrap();
        // k2: no TTL
        mb.deliver(&k2, TaskStatus::Completed, None).unwrap();

        let evicted = mb.evict_expired(u128::MAX).unwrap();
        assert_eq!(evicted, 1);
        assert_eq!(mb.len(), 1);
        assert!(mb.peek(&k2).is_some());
    }

    #[test]
    fn consume_nonexistent_returns_none() {
        let mut mb = InMemoryMailbox::<String>::new();
        assert!(mb.consume(&key("missing")).unwrap().is_none());
    }
}
