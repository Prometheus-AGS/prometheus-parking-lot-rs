//! Result mailbox contract: asynchronous, idempotent delivery of task
//! outcomes to clients that may be disconnected (ADR 0003).
//!
//! # Semantics
//!
//! - **Addressing.** A mailbox holds zero or more [`MailboxEntry`] values per
//!   [`MailboxKey`], in delivery order.
//! - **First write wins within one address.** The idempotency identity is the
//!   pair `(Delivery.key, Delivery.idempotency_key)`, within this mailbox's
//!   storage namespace. All [`MailboxKey`] fields participate: tenant, user
//!   and session, including the distinction between `None` and an empty string.
//!   The same caller key at a different address is independent. A [`Delivery`]
//!   with a pair that the mailbox still remembers stores nothing and resolves to
//!   [`DeliveryOutcome::Duplicate`]; the first stored result stands, even if a
//!   re-execution produced a different value. The duplicate outcome returns
//!   that original stored [`TaskStatus`], including its full failure details,
//!   even after consume; reading duplicate metadata never consumes a result.
//!   A key and its status are remembered until the
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
//! # Upgrading persisted idempotency records
//!
//! The 0.3.0 stores remembered caller keys across the whole storage namespace,
//! without an address on consumed-result tombstones. An upgrade must preserve
//! results, original key lifetimes and consumed-result deduplication. It must
//! not guess an address or silently discard an unexpired tombstone: migration
//! refuses atomically when ownership is not provable, unless an authoritative
//! owner mapping is supplied. Old writers must remain stopped after conversion.
//! Fresh stores and migrated, resolved records obey the address-scoped rules
//! above. ADR 0006 records the backend migration and rollback requirements.
//! Legacy consumed tombstones also lack status. ADR 0007 requires a retained
//! exact-generation proof or [`LegacyDeliveryStatus`] evidence before an atomic
//! status-metadata conversion; neither the retry's status nor `Completed` is
//! a substitute for an unknown historical value.
//!
//! # Notifications
//!
//! A [`Notifier`] tells an external system that an entry was stored (a
//! webhook, an event stream). [`StorageMailbox`] composes a storage backend
//! with a notifier: store first, then notify; a notifier failure never fails
//! or undoes the stored delivery.

use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::contracts::error::SchedulerError;
use crate::contracts::task::TaskStatus;
use crate::contracts::types::{MailboxKey, TaskId};

/// An authoritative owner assignment for an idempotency tombstone written by
/// a legacy store without address provenance.
///
/// Migration consumes these assignments only after the operator has identified
/// the original owner from an external authoritative source. The assignment
/// does not reset the stored key lifetime or create a result. Storage namespaces
/// are supplied separately: a Postgres mailbox name, or the embedded store
/// containing the tombstone. Backends reject duplicate, unknown or conflicting
/// assignments and refuse atomically if any unexpired owner remains unresolved.
///
/// This value contains no terminal-status claim: legacy consumed tombstones
/// do not record status either. A later metadata migration must resolve that
/// separate missing fact rather than inventing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct LegacyIdempotencyOwner {
    /// The exact caller key remembered by the legacy store.
    pub idempotency_key: String,
    /// Its original complete mailbox address.
    pub key: MailboxKey,
}

impl LegacyIdempotencyOwner {
    /// Assign a legacy caller key to its authoritative original address.
    #[must_use]
    pub fn new(idempotency_key: impl Into<String>, key: MailboxKey) -> Self {
        Self {
            idempotency_key: idempotency_key.into(),
            key,
        }
    }
}

/// Authoritative original status evidence for a legacy idempotency fence.
///
/// The identity includes the complete address and caller key: intermediate
/// address-scoped stores can remember the same caller key at multiple addresses.
/// Storage namespaces are supplied separately by each backend's migration API.
/// Conversion retains the original lifetime and records immutable provenance;
/// replay validates evidence without recreating expired fences.
///
/// This type accepts every [`TaskStatus`], including `Queued` and `Running`,
/// just as [`Delivery`] does. A durable consumer cannot treat a nonterminal
/// stored status as an authoritative terminal result: it reports the conflict
/// and requeues rather than fabricating completion or resolving the lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct LegacyDeliveryStatus {
    /// The caller key remembered by the legacy store.
    pub idempotency_key: String,
    /// Its complete original mailbox address.
    pub key: MailboxKey,
    /// The original stored status, obtained from authoritative evidence.
    pub status: TaskStatus,
}

impl LegacyDeliveryStatus {
    /// Associate a legacy address/caller-key fence with its original status.
    #[must_use]
    pub fn new(idempotency_key: impl Into<String>, key: MailboxKey, status: TaskStatus) -> Self {
        Self {
            idempotency_key: idempotency_key.into(),
            key,
            status,
        }
    }
}

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
    /// First-write-wins key scoped to this delivery's complete [`MailboxKey`];
    /// normally the task's
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

    /// Set the first-write-wins idempotency key, scoped to [`Self::key`].
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[must_use]
pub enum DeliveryOutcome {
    /// The entry was stored.
    Stored,
    /// This address and idempotency-key pair was already remembered; nothing
    /// was stored or consumed. The first delivery's status is authoritative.
    Duplicate {
        /// The exact original status, retained until the original key expiry
        /// even after its result was consumed. It is never the retry's status.
        status: TaskStatus,
    },
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
    /// Store `delivery` unless the pair of its complete [`MailboxKey`] and
    /// idempotency key is already remembered in this storage namespace.
    /// A duplicate returns its original stored status atomically with the
    /// duplicate decision, without consuming or peeking at the payload. A
    /// backend that cannot obtain authoritative metadata returns an error;
    /// it must not report a metadata-free success or guess a status.
    ///
    /// **Cancellation:** if dropped, the entry may or may not be stored. A
    /// retry with the same address and idempotency key resolves to `Duplicate`
    /// when the first attempt landed, so callers that retry set a key.
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
