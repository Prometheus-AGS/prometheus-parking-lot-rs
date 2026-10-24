//! Durable queue contract: asynchronous enqueue with lease, ack, nack and
//! extend (ADR 0003).
//!
//! # Lifecycle of a queue entry
//!
//! ```text
//! enqueue ──► ready ──lease──► leased ──ack──────────────► (removed)
//!               ▲                │ │
//!               │                │ └──nack(DeadLetter)───► dead-lettered
//!               │   nack(Requeue │
//!               │   {delay}) or  │
//!               └── lease expiry ┘     (delayed until `delay` elapses)
//! ```
//!
//! - **Durable acceptance.** [`LeaseQueue::enqueue`] resolves `Ok` only once
//!   the task is stored as durably as the backend can store it; a pool may
//!   acknowledge acceptance to its caller after that and not before.
//! - **Leases.** [`LeaseQueue::lease`] hands out up to `max` ready tasks, each
//!   with a [`LeaseToken`]. A leased task is invisible to every other `lease`
//!   call until it is acked, nacked, or its lease expires; after expiry it is
//!   ready again and the next `lease` redelivers it with a higher
//!   [`attempt`](LeasedTask::attempt).
//! - **Fencing.** Each lease gets a fresh token. Once a lease has expired (or
//!   the task was acked or nacked), operations with its token fail with
//!   [`SchedulerError::LeaseLost`], even if the task has not been redelivered
//!   yet, so a slow consumer cannot ack work another consumer now owns.
//! - **Ordering.** Ready tasks are leased by priority (highest first), then in
//!   enqueue order. A task that is requeued or whose lease expires keeps its
//!   original place.
//! - **Delivery is at-least-once.** A consumer that crashes after doing the
//!   work but before acking causes a redelivery; make side effects idempotent
//!   (the mailbox is, through the task's idempotency key — see
//!   [`ResultMailbox`](crate::contracts::ResultMailbox)).
//!
//! # Cancellation safety
//!
//! Every method may be cancelled (its future dropped) at an await point. The
//! contract each backend keeps when that happens is documented per method; in
//! short, no cancellation can lose an accepted task, it can only cause a
//! redelivery or leave an `enqueue` outcome unknown to the caller.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::contracts::error::SchedulerError;
use crate::contracts::task::ScheduledTask;

/// Identifier a queue backend assigns to an entry when it is enqueued.
///
/// Distinct from [`TaskMetadata::id`](crate::contracts::TaskMetadata), which a
/// pool generates per process and which is not unique across restarts. A
/// backend's ids are unique within that queue for as long as the entry exists;
/// [`LeaseQueue::identity`] names the queue, so the pair is unique across
/// queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct QueueEntryId(pub u64);

impl fmt::Display for QueueEntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Proof of one lease on one queue entry; required to ack, nack or extend it.
///
/// A token names the entry and a *fence*: a number the backend assigns to each
/// lease and never reuses for that entry. A token whose fence is not the
/// entry's current, unexpired lease is stale and every operation with it fails
/// with [`SchedulerError::LeaseLost`].
///
/// Tokens are plain data (`Copy`, `Serialize`), so a heartbeat task or another
/// process can extend a lease on the consumer's behalf.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LeaseToken {
    entry: QueueEntryId,
    fence: u64,
}

impl LeaseToken {
    /// A token for lease `fence` on `entry`. Backends construct tokens; callers
    /// only pass them back.
    #[must_use]
    pub const fn new(entry: QueueEntryId, fence: u64) -> Self {
        Self { entry, fence }
    }

    /// The leased entry.
    #[must_use]
    pub const fn entry(&self) -> QueueEntryId {
        self.entry
    }

    /// The lease's fence number.
    #[must_use]
    pub const fn fence(&self) -> u64 {
        self.fence
    }

    /// The [`SchedulerError::LeaseLost`] error for this token.
    #[must_use]
    pub const fn lost(&self) -> SchedulerError {
        SchedulerError::LeaseLost {
            entry: self.entry.0,
        }
    }
}

/// A task handed out by [`LeaseQueue::lease`], with the lease that covers it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LeasedTask<P> {
    /// The task, with the metadata (including its idempotency key) and
    /// payload it was enqueued with.
    pub task: ScheduledTask<P>,
    /// The lease; pass it to ack, nack or extend.
    pub token: LeaseToken,
    /// How many times this entry has been leased, counting this lease: 1 on
    /// first delivery, higher on every redelivery (nack-requeue or lease
    /// expiry). A dispatcher uses it to dead-letter poison tasks.
    pub attempt: u32,
}

impl<P> LeasedTask<P> {
    /// A leased task. Backends construct these.
    #[must_use]
    pub const fn new(task: ScheduledTask<P>, token: LeaseToken, attempt: u32) -> Self {
        Self {
            task,
            token,
            attempt,
        }
    }
}

/// What to do with a leased task that will not be acked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
#[non_exhaustive]
pub enum NackAction {
    /// Make the task ready again after `delay` (zero: immediately). It keeps
    /// its priority and its place in enqueue order, and its next lease is a new
    /// attempt. The not-before time is `now + delay` on the backend's clock.
    Requeue {
        /// Minimum time before the task can be leased again (backoff).
        delay: Duration,
    },
    /// Remove the task from the ready set permanently and keep it as a dead
    /// letter with `reason`. Dead letters are never leased again; they count in
    /// [`QueueDepth::dead_lettered`] but not toward the queue's maximum depth.
    DeadLetter {
        /// Why the task was given up on (for operators).
        reason: String,
    },
}

impl NackAction {
    /// Requeue for immediate redelivery.
    #[must_use]
    pub const fn requeue() -> Self {
        Self::Requeue {
            delay: Duration::ZERO,
        }
    }

    /// Requeue, ready again after `delay`.
    #[must_use]
    pub const fn requeue_after(delay: Duration) -> Self {
        Self::Requeue { delay }
    }

    /// Dead-letter with `reason`.
    pub fn dead_letter(reason: impl Into<String>) -> Self {
        Self::DeadLetter {
            reason: reason.into(),
        }
    }
}

/// Outcome of a successful [`LeaseQueue::enqueue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[must_use]
pub enum Enqueued {
    /// The task was stored as a new entry.
    Accepted(QueueEntryId),
    /// The task carried an idempotency key that an entry still in the queue
    /// (ready, delayed or leased) already has; nothing was stored and this is
    /// that entry. Safe to treat as accepted.
    Duplicate(QueueEntryId),
}

impl Enqueued {
    /// The entry holding the task, whether new or pre-existing.
    #[must_use]
    pub const fn entry(self) -> QueueEntryId {
        match self {
            Self::Accepted(id) | Self::Duplicate(id) => id,
        }
    }
}

/// Snapshot of a queue's contents by state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct QueueDepth {
    /// Entries that the next `lease` could hand out (including entries whose
    /// lease has expired).
    pub ready: usize,
    /// Entries requeued with a delay that has not elapsed yet.
    pub delayed: usize,
    /// Entries under an unexpired lease.
    pub leased: usize,
    /// Dead letters.
    pub dead_lettered: usize,
}

impl QueueDepth {
    /// A depth snapshot. Backends construct these.
    #[must_use]
    pub const fn new(ready: usize, delayed: usize, leased: usize, dead_lettered: usize) -> Self {
        Self {
            ready,
            delayed,
            leased,
            dead_lettered,
        }
    }

    /// Entries that count toward the maximum depth: ready, delayed and leased
    /// (not dead letters).
    #[must_use]
    pub const fn live(&self) -> usize {
        self.ready + self.delayed + self.leased
    }
}

/// An asynchronous, leasing task queue: the contract every durable queue
/// backend (Postgres, pgmq, embedded) and the in-memory reference
/// [`InMemoryLeaseQueue`](crate::contracts::InMemoryLeaseQueue) implement.
///
/// The `lease_queue` module documentation describes the entry lifecycle,
/// fencing and ordering rules; ADR 0003 records why methods return
/// `impl Future + Send`.
///
/// Implementations write `async fn` for each method; the declared `+ Send`
/// makes the compiler check that every implementation's future can move
/// between threads, so generic dispatchers can spawn them on multi-threaded
/// runtimes.
///
/// # Errors
///
/// Methods return [`SchedulerError::Backend`] for storage failures
/// ([`SchedulerError::NotStored`] from `enqueue` when nothing was written),
/// [`SchedulerError::QueueFull`] when `enqueue` finds the queue at its maximum
/// depth, [`SchedulerError::InvalidConfig`] for a zero lease duration, and
/// [`SchedulerError::LeaseLost`] for a stale token.
pub trait LeaseQueue<P: Send>: Send + Sync {
    /// Store `task` as a new ready entry.
    ///
    /// If the task's [`idempotency_key`](crate::contracts::TaskMetadata) is set
    /// and an entry with the same key is still in the queue (ready, delayed or
    /// leased), nothing is stored and [`Enqueued::Duplicate`] names that entry.
    /// Once an entry is acked or dead-lettered its key no longer deduplicates.
    ///
    /// Fails with [`SchedulerError::QueueFull`] when
    /// [`QueueDepth::live`] has reached [`max_depth`](Self::max_depth).
    /// A backend failure that certainly stored nothing (the task cannot be
    /// serialised, no transaction could be opened, a statement failed before
    /// the commit) should be [`SchedulerError::NotStored`], so a pool can
    /// report the task as refused; [`SchedulerError::Backend`] means the
    /// outcome is unknown (the task may be stored).
    ///
    /// **Cancellation:** if the future is dropped before it resolves, the task
    /// may or may not have been stored. Callers that must not lose or duplicate
    /// work set an idempotency key and retry; the retry resolves to `Duplicate`
    /// when the first attempt landed.
    fn enqueue(
        &self,
        task: ScheduledTask<P>,
    ) -> impl Future<Output = Result<Enqueued, SchedulerError>> + Send;

    /// Lease up to `max` ready tasks for `lease_for`, highest priority first
    /// and in enqueue order within a priority. Returns immediately with an
    /// empty vector when nothing is ready (it does not wait for work).
    ///
    /// Each returned task is invisible to other `lease` calls until it is
    /// acked, nacked, or `lease_for` elapses on the backend's clock.
    /// `max == 0` returns an empty vector. A zero `lease_for` fails with
    /// [`SchedulerError::InvalidConfig`].
    ///
    /// **Cancellation:** if the future is dropped after the backend recorded
    /// the leases, those tasks stay leased to nobody until the leases expire
    /// and are then redelivered. Nothing is lost; delivery is delayed by at
    /// most `lease_for`.
    fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> impl Future<Output = Result<Vec<LeasedTask<P>>, SchedulerError>> + Send;

    /// Remove a leased task permanently: its work is done (and its result
    /// delivered). Fails with [`SchedulerError::LeaseLost`] when `token` is not
    /// the entry's current, unexpired lease.
    ///
    /// **Cancellation:** if dropped, the ack may or may not have applied; if it
    /// did not, the task is redelivered after the lease expires (at-least-once).
    fn ack(&self, token: LeaseToken) -> impl Future<Output = Result<(), SchedulerError>> + Send;

    /// Give a leased task back: requeue it (optionally delayed) or dead-letter
    /// it, per `action`. Fails with [`SchedulerError::LeaseLost`] when `token`
    /// is not the entry's current, unexpired lease.
    ///
    /// **Cancellation:** if dropped, the nack may or may not have applied; if it
    /// did not, the lease simply expires and the task is redelivered.
    fn nack(
        &self,
        token: LeaseToken,
        action: NackAction,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send;

    /// Renew a lease so it now expires `lease_for` from now (not from the old
    /// expiry). Long-running consumers call this periodically as a heartbeat,
    /// well inside the lease duration. Fails with
    /// [`SchedulerError::LeaseLost`] when `token` is not the entry's current,
    /// unexpired lease (an expired lease cannot be revived), and with
    /// [`SchedulerError::InvalidConfig`] for a zero `lease_for`.
    ///
    /// **Cancellation:** if dropped, the lease keeps whichever expiry it had;
    /// heartbeat again or treat the lease as lost when it would have expired.
    fn extend(
        &self,
        token: LeaseToken,
        lease_for: Duration,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send;

    /// Count the entries by state. The snapshot may be stale by the time it is
    /// read when other consumers are active.
    ///
    /// **Cancellation:** read-only; dropping it has no effect.
    fn depth(&self) -> impl Future<Output = Result<QueueDepth, SchedulerError>> + Send;

    /// Maximum number of live (ready, delayed or leased) entries `enqueue`
    /// accepts.
    fn max_depth(&self) -> usize;

    /// A stable name for the queue this handle reads and writes, which
    /// qualifies its [`QueueEntryId`]s.
    ///
    /// Entry ids are unique only within one queue, so a consumer that keys
    /// anything on an entry id (a durable pool keys each result's idempotency
    /// key as `"<identity>:entry:<entry id>"`) qualifies it with this name.
    /// The identity must be:
    ///
    /// - **the same** for every handle on the same queue, in every process
    ///   and across restarts, for as long as the queue exists, so every
    ///   redelivery of an entry is recognised whoever consumes it;
    /// - **different** for any two queues whose entry ids could repeat, so
    ///   results of two queues that share one mailbox never collide.
    ///
    /// It does not depend on how a consumer names itself. The built-in
    /// backends use `in-memory:<process nonce>:<instance>` (one identity per
    /// `InMemoryLeaseQueue` value), `pg:<table>:<queue name>`,
    /// `pgmq:<queue name>` and `redb:<store id>` (an id generated when the
    /// database file is created and stored in it).
    ///
    /// **Limit:** a queue that is dropped and recreated under the same
    /// identity (a Postgres table or pgmq queue recreated, so its id sequence
    /// restarts) can hand out an entry id again; a mailbox that still
    /// remembers the old entry's key would then treat the new entry's result
    /// as a duplicate. Recreate the mailbox with the queue, or set
    /// [`idempotency_key`](crate::contracts::TaskMetadata) on every task.
    fn identity(&self) -> &str;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_round_trips_and_names_its_entry() {
        let token = LeaseToken::new(QueueEntryId(7), 3);
        let json = serde_json::to_string(&token).expect("serialize");
        let back: LeaseToken = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, token);
        assert!(matches!(
            token.lost(),
            SchedulerError::LeaseLost { entry: 7 }
        ));
        assert!(!token.lost().is_retryable());
    }

    #[test]
    fn nack_action_constructors() {
        assert_eq!(
            NackAction::requeue(),
            NackAction::Requeue {
                delay: Duration::ZERO
            }
        );
        let json = serde_json::to_string(&NackAction::dead_letter("poison")).expect("serialize");
        assert_eq!(json, r#"{"action":"dead_letter","reason":"poison"}"#);
    }

    #[test]
    fn live_depth_excludes_dead_letters() {
        assert_eq!(QueueDepth::new(1, 2, 3, 100).live(), 6);
        assert_eq!(
            Enqueued::Duplicate(QueueEntryId(4)).entry(),
            QueueEntryId(4)
        );
    }
}
