//! The shared redb database handle behind the queue and the mailbox.

use std::error::Error;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Clock, LegacyDeliveryStatus, LegacyIdempotencyOwner, SchedulerError, SystemClock,
};
use redb::{Database, WriteTransaction};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::fault::{self, Phase};
use crate::mailbox::{self, RedbResultMailbox};
use crate::queue::{self, RedbLeaseQueue};

/// Version of the on-disk layout; stored in the database and checked on open.
const SCHEMA_VERSION: u64 = 3;

/// Name of the `schema_version` row in the queue's metadata table.
const SCHEMA_KEY: &str = "schema_version";

/// Names of the two rows holding the store's random 128-bit id, generated
/// when the file is first opened: the queue's `LeaseQueue::identity`.
const STORE_ID_KEYS: [&str; 2] = ["store_id_hi", "store_id_lo"];

/// An open redb database file that hosts a [`RedbLeaseQueue`] and a
/// [`RedbResultMailbox`].
///
/// redb allows one open handle per file, so the queue and the mailbox of one
/// application share a store: clone it freely (it is a pair of `Arc`s).
#[derive(Debug, Clone)]
pub struct EmbeddedStore {
    pub(crate) db: Arc<Database>,
    pub(crate) clock: Arc<dyn Clock>,
    /// `redb:<store id>`, the lease queue's identity.
    pub(crate) identity: Arc<str>,
}

impl EmbeddedStore {
    /// Open (or create) the database file at `path`, creating missing parent
    /// directories, timed by the system clock.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the file cannot be created or opened (for
    /// example another process holds it open), is not a redb database, or was
    /// written by an incompatible version of this crate. Incompatible layouts
    /// and owner mappings return [`SchedulerError::InvalidConfig`].
    /// Schema-1 and schema-2 stores require explicit conversion through
    /// [`open_with_idempotency_metadata`](Self::open_with_idempotency_metadata).
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SchedulerError> {
        Self::open_with_clock(path, SystemClock::new())
    }

    /// Like [`open`](Self::open), with leases and TTLs timed by `clock`
    /// (tests drive time with a `TestClock`).
    ///
    /// # Errors
    ///
    /// As for [`open`](Self::open).
    pub fn open_with_clock(
        path: impl AsRef<Path>,
        clock: impl Clock + 'static,
    ) -> Result<Self, SchedulerError> {
        Self::open_internal(path.as_ref(), clock, None, &[])
    }

    /// Explicitly convert a legacy store using owner evidence only.
    ///
    /// Stop all old readers/writers and work on an isolated copy with an
    /// untouched backup. `owners` must authoritatively identify consumed or
    /// ambiguous unexpired legacy fences. Original status must be provable from
    /// the exact retained generation; consumed fences with unknown status need
    /// [`open_with_idempotency_metadata`](Self::open_with_idempotency_metadata).
    /// Conversion preserves all results, expiry times,
    /// legacy fences, sequence counters and queue identity in one transaction.
    /// Old binaries reject the resulting layout. Rollback requires restoring
    /// the backup with all new writers stopped; it loses subsequent new writes.
    ///
    /// # Errors
    ///
    /// As for [`open`](Self::open); also refuses missing, duplicate, unknown,
    /// expired or conflicting owner assignments. Refusal leaves logical store
    /// contents unchanged. A repeated conversion validates supplied owners
    /// against immutable provenance and never resets an active fence's TTL.
    pub fn open_with_idempotency_owners(
        path: impl AsRef<Path>,
        owners: &[LegacyIdempotencyOwner],
    ) -> Result<Self, SchedulerError> {
        Self::open_with_clock_and_idempotency_owners(path, SystemClock::new(), owners)
    }

    /// Like [`open_with_idempotency_owners`](Self::open_with_idempotency_owners),
    /// with leases and TTLs timed by `clock`.
    ///
    /// # Errors
    ///
    /// As for [`open_with_idempotency_owners`](Self::open_with_idempotency_owners).
    pub fn open_with_clock_and_idempotency_owners(
        path: impl AsRef<Path>,
        clock: impl Clock + 'static,
        owners: &[LegacyIdempotencyOwner],
    ) -> Result<Self, SchedulerError> {
        Self::open_with_clock_and_idempotency_metadata(path, clock, owners, &[])
    }

    /// Convert schema 1 or 2 to schema 3 using authoritative original metadata,
    /// or reopen schema 3 while validating any supplied evidence.
    ///
    /// `owners` resolves schema-1 addresses; `statuses` separately resolves the
    /// original full status when no retained result proves it. Schema-2 fences
    /// already carry addresses. Every conversion is one transaction, including
    /// the version marker, and retains raw results, queue state/identity, TTLs,
    /// and immutable legacy provenance. No status is taken from a retry and no
    /// result is consumed to discover metadata. Queued/Running are preserved.
    ///
    /// Stop old readers/writers, keep an untouched backup, and rehearse on a
    /// copy. Both old writer versions reject schema 3. Rollback restores the
    /// backup with new writers stopped and requires reconciling later writes.
    ///
    /// # Errors
    ///
    /// As for [`open`](Self::open). Missing, duplicate, unknown, expired or
    /// contradictory initial evidence refuses conversion with no logical
    /// changes. Schema-3 replays validate immutable ownership/status evidence
    /// without rebuilding active fences or extending their original lifetime.
    pub fn open_with_idempotency_metadata(
        path: impl AsRef<Path>,
        owners: &[LegacyIdempotencyOwner],
        statuses: &[LegacyDeliveryStatus],
    ) -> Result<Self, SchedulerError> {
        Self::open_with_clock_and_idempotency_metadata(path, SystemClock::new(), owners, statuses)
    }

    /// Like [`open_with_idempotency_metadata`](Self::open_with_idempotency_metadata),
    /// with leases and TTLs timed by `clock`.
    ///
    /// # Errors
    ///
    /// As for [`open_with_idempotency_metadata`](Self::open_with_idempotency_metadata).
    pub fn open_with_clock_and_idempotency_metadata(
        path: impl AsRef<Path>,
        clock: impl Clock + 'static,
        owners: &[LegacyIdempotencyOwner],
        statuses: &[LegacyDeliveryStatus],
    ) -> Result<Self, SchedulerError> {
        Self::open_internal(path.as_ref(), clock, Some(owners), statuses)
    }

    fn open_internal(
        path: &Path,
        clock: impl Clock + 'static,
        owners: Option<&[LegacyIdempotencyOwner]>,
        statuses: &[LegacyDeliveryStatus],
    ) -> Result<Self, SchedulerError> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(backend("embedded::open::create_dir"))?;
        }
        let db = Database::create(path).map_err(backend("embedded::open"))?;
        let mut store = Self {
            db: Arc::new(db),
            clock: Arc::new(clock),
            identity: Arc::from(""),
        };
        store.identity = store.initialise(owners, statuses)?;
        Ok(store)
    }

    /// A lease queue holding at most `max_depth` live entries in this store.
    #[must_use]
    pub fn lease_queue<P>(&self, max_depth: usize) -> RedbLeaseQueue<P>
    where
        P: Serialize + DeserializeOwned + Send + 'static,
    {
        RedbLeaseQueue::new(self.clone(), max_depth)
    }

    /// A result mailbox in this store.
    #[must_use]
    pub fn result_mailbox<T>(&self) -> RedbResultMailbox<T>
    where
        T: Serialize + DeserializeOwned + Send + 'static,
    {
        RedbResultMailbox::new(self.clone())
    }

    /// Create every table (readers cannot open a table that does not exist yet),
    /// check the layout version and read (or create) the store id, in one
    /// transaction. Returns the queue identity.
    fn initialise(
        &self,
        owners: Option<&[LegacyIdempotencyOwner]>,
        statuses: &[LegacyDeliveryStatus],
    ) -> Result<Arc<str>, SchedulerError> {
        let txn = self
            .db
            .begin_write()
            .map_err(backend("embedded::open::begin"))?;
        queue::create_tables(&txn)?;
        mailbox::create_tables(&txn)?;
        let found = queue::meta_get(&txn, SCHEMA_KEY)?;
        match found {
            None => {
                mailbox::validate_idempotency_owners(&txn, owners.unwrap_or_default())?;
                mailbox::validate_metadata(&txn, statuses)?;
                queue::meta_set(&txn, SCHEMA_KEY, SCHEMA_VERSION)?;
            }
            Some(SCHEMA_VERSION) => {
                mailbox::validate_idempotency_owners(&txn, owners.unwrap_or_default())?;
                mailbox::validate_metadata(&txn, statuses)?;
            }
            Some(1 | 2) => {
                let owners = owners.ok_or_else(|| {
                    SchedulerError::invalid_config(
                        "embedded schema 1 or 2 requires explicit open_with_idempotency_metadata conversion",
                    )
                })?;
                let now = now_ms(&*self.clock);
                if found == Some(1) {
                    mailbox::migrate_idempotency(&txn, now, owners)?;
                } else {
                    mailbox::validate_idempotency_owners(&txn, owners)?;
                }
                mailbox::migrate_metadata(&txn, now, statuses)?;
                queue::meta_set(&txn, SCHEMA_KEY, SCHEMA_VERSION)?;
            }
            Some(other) => {
                return Err(SchedulerError::invalid_config(format!(
                    "embedded store has layout version {other}, this build understands {SCHEMA_VERSION}"
                )));
            }
        }
        let mut id = [0_u64; 2];
        for (part, name) in id.iter_mut().zip(STORE_ID_KEYS) {
            *part = if let Some(value) = queue::meta_get(&txn, name)? {
                value
            } else {
                let value = random_u64();
                queue::meta_set(&txn, name, value)?;
                value
            };
        }
        if matches!(found, Some(1 | 2)) {
            commit(txn, "migrate_mailbox_metadata")?;
        } else {
            txn.commit().map_err(backend("embedded::open::commit"))?;
        }
        Ok(Arc::from(format!("redb:{:016x}{:016x}", id[0], id[1])))
    }

    /// Run `work` on the blocking pool with the database and the clock.
    ///
    /// If the returned future is dropped, `work` still runs to completion, so a
    /// started transaction is committed or rolled back, never left half done.
    pub(crate) async fn blocking<R, F>(&self, work: F) -> Result<R, SchedulerError>
    where
        R: Send + 'static,
        F: FnOnce(&Database, &dyn Clock) -> Result<R, SchedulerError> + Send + 'static,
    {
        let db = Arc::clone(&self.db);
        let clock = Arc::clone(&self.clock);
        tokio::task::spawn_blocking(move || work(&db, &*clock))
            .await
            .map_err(|e| SchedulerError::internal(format!("embedded store task failed: {e}")))?
    }
}

/// A random `u64` from the standard library's per-process hash keys, mixed
/// with the wall clock and the process id (no extra dependency).
fn random_u64() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    if let Ok(since_epoch) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        hasher.write_u128(since_epoch.as_nanos());
    }
    hasher.write_u32(std::process::id());
    hasher.finish()
}

/// Commit `txn`, running the crash-point hooks around the commit.
///
/// Every state change in this crate ends here, so "committed durably before the
/// caller is told" is enforced in one place.
pub fn commit(txn: WriteTransaction, operation: &'static str) -> Result<(), SchedulerError> {
    fault::point(operation, Phase::BeforeCommit);
    // Negative control for scenario S-8: acknowledge an enqueue without
    // committing it. The crash-point tests must catch the lost task.
    #[cfg(parking_lot_negative_control = "s-8")]
    if operation == "enqueue" {
        drop(txn);
        fault::point(operation, Phase::AfterCommit);
        return Ok(());
    }
    txn.commit().map_err(backend("embedded::commit"))?;
    fault::point(operation, Phase::AfterCommit);
    Ok(())
}

/// Wrap a storage error as [`SchedulerError::Backend`] with `context`.
pub fn backend<E>(context: &'static str) -> impl FnOnce(E) -> SchedulerError
where
    E: Error + Send + Sync + 'static,
{
    move |source| SchedulerError::backend(context, source)
}

/// Wall-clock milliseconds as stored on disk.
pub fn now_ms(clock: &dyn Clock) -> u64 {
    u64::try_from(clock.now_ms()).unwrap_or(u64::MAX)
}

/// A duration in whole milliseconds, rounded up so a non-zero duration is never
/// stored as zero.
pub fn duration_ms(duration: Duration) -> u64 {
    let whole = duration.as_millis();
    let rounded = if duration.subsec_nanos().is_multiple_of(1_000_000) {
        whole
    } else {
        whole + 1
    };
    u64::try_from(rounded).unwrap_or(u64::MAX)
}
