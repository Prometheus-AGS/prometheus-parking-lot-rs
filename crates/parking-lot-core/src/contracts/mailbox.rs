//! Mailbox backend contract.

use crate::contracts::error::SchedulerError;
use crate::contracts::task::TaskStatus;
use crate::contracts::types::MailboxKey;

/// Abstraction for mailbox backends.
///
/// ## Delivery semantics
///
/// Delivery is **at-least-once** for persistent backends. Callers should set an
/// idempotency key on the task to detect redeliveries. The `consume` operation
/// is **consume-once**: after a successful consume the message is removed and
/// will not be returned again.
///
/// ## Notifier failure isolation
///
/// Backends with HTTP/push notifiers must store the result before invoking
/// the notifier. A notifier failure must **not** fail the deliver call or lose
/// the stored result; it should be logged and retried separately.
///
/// ## Expiry
///
/// Backends should respect the optional TTL from `deliver_with_ttl`. Messages
/// older than their TTL may be evicted during `evict_expired`.
pub trait Mailbox<T> {
    /// Deliver a task outcome to the mailbox.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails to store the outcome.
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<T>,
    ) -> Result<(), SchedulerError>;

    /// Deliver with an explicit TTL in milliseconds.
    ///
    /// The default implementation ignores the TTL and delegates to `deliver`.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails to store the outcome.
    fn deliver_with_ttl(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<T>,
        _ttl_ms: u64,
    ) -> Result<(), SchedulerError> {
        self.deliver(key, status, payload)
    }

    /// Consume (take and remove) the message for `key`, if present.
    ///
    /// Returns `None` if no message exists. After a successful consume the
    /// message is removed (consume-once semantics).
    ///
    /// The default implementation returns `None` (not supported by backend).
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails to read or remove the message.
    fn consume(
        &mut self,
        _key: &MailboxKey,
    ) -> Result<Option<(TaskStatus, Option<T>)>, SchedulerError> {
        Ok(None)
    }

    /// Evict messages whose TTL has expired.
    ///
    /// Returns the number of messages evicted. The default is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails to remove expired messages.
    fn evict_expired(&mut self, _now_ms: u128) -> Result<usize, SchedulerError> {
        Ok(0)
    }
}
