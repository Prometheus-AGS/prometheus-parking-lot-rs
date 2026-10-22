//! Result mailbox contract: asynchronous, idempotent delivery of task
//! outcomes to clients that may be disconnected (ADR 0003).
//!
//! # Semantics
//!
//! - **Addressing.** A mailbox holds zero or more [`MailboxEntry`] values per
//!   [`MailboxKey`], in delivery order.
//! - **First write wins.** A [`Delivery`] with an idempotency key that the
//!   mailbox still remembers stores nothing and resolves to
//!   [`DeliveryOutcome::Duplicate`]; the first stored result stands, even if a
//!   re-execution produced a different value. A key is remembered until the
//!   entry it arrived with is evicted by its TTL, **including after that entry
//!   was consumed**, so a task redelivered by its queue after the client
//!   already took the result cannot hand the client a second one.
//! - **Consume once.** [`ResultMailbox::consume`] takes and removes the oldest
//!   entry for a key; no later call returns it again. [`ResultMailbox::peek`]
//!   reads it without removing it.
//! - **TTL.** A delivery's optional TTL starts when it is stored. Once it has
//!   elapsed on the backend's clock, the entry is never returned again and
//!   [`ResultMailbox::evict_expired`] removes it (and forgets its idempotency
//!   key). Entries without a TTL are kept until consumed; their idempotency
//!   keys are kept for good.
//!
//! The scheduler's durable dispatch (cr-49) delivers a task's outcome **before**
//! acking the task's lease, and sets the delivery's idempotency key from the
//! task, so a crash between the two re-runs the task without duplicating its
//! result.
//!
//! # Notifications
//!
//! A [`Notifier`] tells an external system that an entry was stored (a
//! webhook, an event stream). [`StorageMailbox`] composes a storage backend
//! with a notifier: store first, then notify; a notifier failure never fails
//! or undoes the stored delivery.

use std::future::Future;
use std::time::Duration;

use crate::contracts::error::SchedulerError;
use crate::contracts::task::TaskStatus;
use crate::contracts::types::{MailboxKey, TaskId};

/// A task outcome to store in a mailbox.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Delivery<T> {
    /// Where the entry is stored.
    pub key: MailboxKey,
    /// The task the outcome belongs to.
    pub task_id: TaskId,
    /// The task's final (or current) status.
    pub status: TaskStatus,
    /// The task's result, when it produced one.
    pub payload: Option<T>,
    /// First-write-wins key; normally the task's
    /// [`idempotency_key`](crate::contracts::TaskMetadata). `None` stores
    /// unconditionally.
    pub idempotency_key: Option<String>,
    /// How long the entry is kept after it is stored; `None` keeps it until
    /// it is consumed.
    pub ttl: Option<Duration>,
}

impl<T> Delivery<T> {
    /// A delivery with no idempotency key and no TTL.
    #[must_use]
    pub const fn new(
        key: MailboxKey,
        task_id: TaskId,
        status: TaskStatus,
        payload: Option<T>,
    ) -> Self {
        Self {
            key,
            task_id,
            status,
            payload,
            idempotency_key: None,
            ttl: None,
        }
    }

    /// Set the first-write-wins idempotency key.
    #[must_use]
    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    /// Set the time to live.
    #[must_use]
    pub const fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }
}

/// An entry stored in a mailbox, as [`ResultMailbox::consume`] and
/// [`ResultMailbox::peek`] return it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MailboxEntry<T> {
    /// The task the outcome belongs to.
    pub task_id: TaskId,
    /// The delivered status.
    pub status: TaskStatus,
    /// The delivered result, if any.
    pub payload: Option<T>,
    /// The delivery's idempotency key, if it had one.
    pub idempotency_key: Option<String>,
    /// Wall-clock milliseconds since the Unix epoch when the entry was stored.
    pub delivered_at_ms: u128,
}

impl<T> MailboxEntry<T> {
    /// The entry a backend stores for `delivery` at wall time
    /// `delivered_at_ms`. The delivery's key and TTL are the backend's to keep
    /// alongside.
    #[must_use]
    pub fn from_delivery(delivery: Delivery<T>, delivered_at_ms: u128) -> Self {
        Self {
            task_id: delivery.task_id,
            status: delivery.status,
            payload: delivery.payload,
            idempotency_key: delivery.idempotency_key,
            delivered_at_ms,
        }
    }

    /// An entry from its parts (for backends that load stored rows).
    #[must_use]
    pub const fn new(
        task_id: TaskId,
        status: TaskStatus,
        payload: Option<T>,
        idempotency_key: Option<String>,
        delivered_at_ms: u128,
    ) -> Self {
        Self {
            task_id,
            status,
            payload,
            idempotency_key,
            delivered_at_ms,
        }
    }
}

/// Outcome of a successful [`ResultMailbox::deliver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[must_use]
pub enum DeliveryOutcome {
    /// The entry was stored.
    Stored,
    /// The idempotency key was already remembered; nothing was stored.
    Duplicate,
}

/// An asynchronous result mailbox: the contract every mailbox storage backend
/// (Postgres, embedded) and the in-memory reference
/// [`InMemoryResultMailbox`](crate::contracts::InMemoryResultMailbox)
/// implement, and that [`StorageMailbox`] composes.
///
/// The `result_mailbox` module documentation states the addressing,
/// first-write-wins, consume-once and TTL rules. Methods return
/// `impl Future + Send` for the reasons in ADR 0003.
///
/// # Errors
///
/// Methods return [`SchedulerError::Backend`] for storage failures.
pub trait ResultMailbox<T: Send>: Send + Sync {
    /// Store `delivery` unless its idempotency key is already remembered.
    ///
    /// **Cancellation:** if dropped, the entry may or may not be stored. A
    /// retry with the same idempotency key resolves to `Duplicate` when the
    /// first attempt landed, so callers that retry set a key.
    fn deliver(
        &self,
        delivery: Delivery<T>,
    ) -> impl Future<Output = Result<DeliveryOutcome, SchedulerError>> + Send;

    /// Take and remove the oldest unexpired entry for `key`; `None` when there
    /// is none.
    ///
    /// **Cancellation:** if dropped after the backend removed the entry, the
    /// entry is gone without having been returned. Backends make the removal
    /// and the read one atomic step (for example `DELETE ... RETURNING`), so
    /// dropping before the step leaves the entry in place; callers that cannot
    /// tolerate the narrow window after it do not race `consume` against a
    /// timeout.
    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send;

    /// Read the oldest unexpired entry for `key` without removing it.
    ///
    /// **Cancellation:** read-only; dropping it has no effect.
    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send;

    /// Remove every entry whose TTL has elapsed and forget its idempotency key;
    /// returns how many entries were removed (consumed entries whose keys are
    /// forgotten are not counted).
    ///
    /// **Cancellation:** eviction is idempotent; a dropped call is finished by
    /// the next one.
    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send;
}

/// Tells an external system that a mailbox entry was stored.
///
/// Notification is best effort: an error is reported to the caller (a
/// [`StorageMailbox`] logs it and may retry) but never undoes or fails the
/// stored delivery.
///
/// # Errors
///
/// [`SchedulerError::Backend`] (or another variant) describing why the
/// notification could not be sent.
pub trait Notifier<T: Sync>: Send + Sync {
    /// Notify that `entry` was stored under `key`.
    ///
    /// **Cancellation:** a dropped notification may or may not have been
    /// sent; receivers must tolerate duplicates and gaps.
    fn notify(
        &self,
        key: &MailboxKey,
        entry: &MailboxEntry<T>,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send;
}

/// A notifier that sends nothing: the default for a [`StorageMailbox`] that
/// only stores.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoopNotifier;

impl<T: Sync> Notifier<T> for NoopNotifier {
    fn notify(
        &self,
        _key: &MailboxKey,
        _entry: &MailboxEntry<T>,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        std::future::ready(Ok(()))
    }
}

/// A mailbox composed of a storage backend `S` and a notifier `N`.
///
/// Its behaviour (the [`ResultMailbox`] implementation: store first, notify
/// after a `Stored` outcome only, notifier failure logged and surfaced but never
/// fatal to the stored delivery, bounded retries) is cr-34 and lives in the
/// `mailbox` module; this contract fixes only the type so pools and builders
/// can name it.
#[derive(Debug, Clone, Default)]
pub struct StorageMailbox<S, N = NoopNotifier> {
    storage: S,
    notifier: N,
}

impl<S> StorageMailbox<S> {
    /// A mailbox that stores in `storage` and notifies nobody.
    #[must_use]
    pub const fn storage_only(storage: S) -> Self {
        Self {
            storage,
            notifier: NoopNotifier,
        }
    }
}

impl<S, N> StorageMailbox<S, N> {
    /// A mailbox that stores in `storage` and then notifies `notifier`.
    #[must_use]
    pub const fn new(storage: S, notifier: N) -> Self {
        Self { storage, notifier }
    }

    /// The storage backend.
    #[must_use]
    pub const fn storage(&self) -> &S {
        &self.storage
    }

    /// The notifier.
    #[must_use]
    pub const fn notifier(&self) -> &N {
        &self.notifier
    }

    /// Split into the storage backend and the notifier.
    #[must_use]
    pub fn into_parts(self) -> (S, N) {
        (self.storage, self.notifier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_builder_sets_key_and_ttl() {
        let key = MailboxKey {
            tenant: "t".into(),
            user_id: None,
            session_id: None,
        };
        let delivery = Delivery::new(key, 9, TaskStatus::Completed, Some(1_u8))
            .with_idempotency_key("job-9")
            .with_ttl(Duration::from_secs(5));
        assert_eq!(delivery.idempotency_key.as_deref(), Some("job-9"));
        assert_eq!(delivery.ttl, Some(Duration::from_secs(5)));

        let entry = MailboxEntry::from_delivery(delivery, 42);
        assert_eq!(entry.task_id, 9);
        assert_eq!(entry.payload, Some(1));
        assert_eq!(entry.delivered_at_ms, 42);
    }

    #[test]
    fn storage_mailbox_holds_its_parts() {
        let mailbox = StorageMailbox::storage_only(7_u8);
        assert_eq!(*mailbox.storage(), 7);
        assert_eq!(*mailbox.notifier(), NoopNotifier);
        assert_eq!(mailbox.into_parts(), (7, NoopNotifier));
    }
}
