//! In-memory mailbox backend.

use std::collections::HashMap;

use crate::core::{Mailbox, TaskStatus};
use crate::core::SchedulerError;
use crate::util::serde::MailboxKey;

/// Mailbox message container.
#[derive(Debug, Clone)]
pub struct MailboxMessage<P> {
    /// Task status.
    pub status: TaskStatus,
    /// Optional payload/result.
    pub payload: Option<P>,
    /// Timestamp milliseconds.
    pub created_at_ms: u128,
}

/// Simple in-memory mailbox for development/testing.
pub struct InMemoryMailbox<P> {
    messages: HashMap<MailboxKey, Vec<MailboxMessage<P>>>,
}

impl<P> InMemoryMailbox<P> {
    /// Create a new mailbox.
    pub fn new() -> Self {
        Self {
            messages: HashMap::new(),
        }
    }

    /// Fetch messages for a mailbox key, optionally since a timestamp.
    pub fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<P>>
    where
        P: Clone,
    {
        self.messages
            .get(key)
            .map(|msgs| {
                msgs.iter()
                    .filter(|m| since_ms.map(|s| m.created_at_ms >= s).unwrap_or(true))
                    .take(limit)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl<P> Mailbox<P> for InMemoryMailbox<P> {
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<P>,
    ) -> Result<(), SchedulerError> {
        let entry = self.messages.entry(key.clone()).or_default();
        entry.push(MailboxMessage {
            status,
            payload,
            created_at_ms: crate::util::clock::now_ms(),
        });
        Ok(())
    }
}
