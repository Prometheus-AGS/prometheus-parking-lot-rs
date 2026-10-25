//! [`RedbResultMailbox`]: a crash-safe [`ResultMailbox`] on redb (ADR 0004).
//!
//! # Layout
//!
//! | table | key | value | holds |
//! |---|---|---|---|
//! | `mb_entries` | (mailbox key JSON, sequence) | entry JSON | undelivered results, in delivery order |
//! | `mb_idem_v3` | (mailbox key JSON, caller idempotency key) | original status and forget-at JSON | active address-scoped fences, kept past consume |
//! | `mb_idem_v2` | (mailbox key JSON, caller idempotency key) | original forget-at ms | immutable schema-2 provenance |
//! | `mb_idem` | legacy caller key | original forget-at ms | immutable schema-1 provenance |
//! | `mb_legacy_owners_v2` | legacy caller key | owner mailbox key JSON | immutable conversion ownership proof |
//! | `mb_legacy_metadata_v3` | (mailbox key JSON, caller idempotency key) | resolved original status and expiry JSON | immutable conversion metadata proof |
//! | `mb_meta` | name | counter | next sequence |
//!
//! `deliver`, `consume` and `evict_expired` are each one write transaction.
//! Because `consume` reads and removes the entry in the same transaction, two
//! consumers can never both receive it, and a crash leaves the entry either
//! still stored or consumed, never lost without having been returned (the
//! returned value only exists after the commit).
//!
//! If the stored payload no longer decodes as `T` (for example after a type
//! change between deploys), `consume` removes that entry in the same transaction
//! and returns the decode error once; the next `consume` of the key sees the
//! entries behind it. This matches the Postgres mailbox. `peek` only reads: it
//! returns the error and leaves the entry for `consume` to discard.

use std::collections::BTreeMap;
use std::future::Future;
use std::marker::PhantomData;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, LegacyDeliveryStatus, LegacyIdempotencyOwner, MailboxEntry,
    MailboxKey, ResultMailbox, SchedulerError, TaskId, TaskStatus,
};
use redb::{ReadableDatabase, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::store::{EmbeddedStore, backend, commit, duration_ms, now_ms};

const NEXT_SEQUENCE: &str = "next_sequence";

const ENTRIES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("mb_entries");
const IDEM: TableDefinition<(&str, &str), &[u8]> = TableDefinition::new("mb_idem_v3");
const SCOPED_V2: TableDefinition<(&str, &str), u64> = TableDefinition::new("mb_idem_v2");
const LEGACY_METADATA: TableDefinition<(&str, &str), &[u8]> =
    TableDefinition::new("mb_legacy_metadata_v3");
const LEGACY_IDEM: TableDefinition<&str, u64> = TableDefinition::new("mb_idem");
const LEGACY_OWNERS: TableDefinition<&str, &str> = TableDefinition::new("mb_legacy_owners_v2");
const META: TableDefinition<&str, u64> = TableDefinition::new("mb_meta");

/// The stored form of a delivered result.
#[derive(Debug, Serialize, Deserialize)]
struct Stored<T> {
    task_id: TaskId,
    status: TaskStatus,
    payload: Option<T>,
    idempotency_key: Option<String>,
    delivered_at_ms: u128,
    /// Wall ms at which the entry stops being returned; `None`: never.
    expires_at_ms: Option<u64>,
}

impl<T> Stored<T> {
    fn into_entry(self) -> MailboxEntry<T> {
        MailboxEntry::new(
            self.task_id,
            self.status,
            self.payload,
            self.idempotency_key,
            self.delivered_at_ms,
        )
    }
}

/// Just the expiry of a stored entry, so scans need not decode payloads.
#[derive(Debug, Deserialize)]
struct Expiry {
    expires_at_ms: Option<u64>,
}

fn is_expired(expires_at_ms: Option<u64>, now: u64) -> bool {
    expires_at_ms.is_some_and(|at| at <= now)
}

fn expiry_of(bytes: &[u8]) -> Result<Option<u64>, SchedulerError> {
    let expiry: Expiry =
        serde_json::from_slice(bytes).map_err(backend("embedded::mailbox::decode_expiry"))?;
    Ok(expiry.expires_at_ms)
}

/// Create the mailbox tables in `txn`.
pub(crate) fn create_tables(txn: &WriteTransaction) -> Result<(), SchedulerError> {
    let context = "embedded::mailbox::create_tables";
    txn.open_table(ENTRIES).map_err(backend(context))?;
    txn.open_table(IDEM).map_err(backend(context))?;
    txn.open_table(SCOPED_V2).map_err(backend(context))?;
    txn.open_table(LEGACY_METADATA).map_err(backend(context))?;
    txn.open_table(LEGACY_IDEM).map_err(backend(context))?;
    txn.open_table(LEGACY_OWNERS).map_err(backend(context))?;
    txn.open_table(META).map_err(backend(context))?;
    Ok(())
}

/// The canonical text form of a mailbox key (its JSON).
fn key_text(key: &MailboxKey) -> Result<String, SchedulerError> {
    serde_json::to_string(key).map_err(backend("embedded::mailbox::encode_key"))
}

#[cfg(not(parking_lot_negative_control = "oa-06"))]
const fn idempotency_address(key: &str) -> &str {
    key
}

/// OA-06 negative control: every address shares the old global caller fence.
#[cfg(parking_lot_negative_control = "oa-06")]
const fn idempotency_address(_key: &str) -> &str {
    ""
}

#[cfg(not(parking_lot_negative_control = "oa-07"))]
const fn duplicate_status(original: TaskStatus, _retry: &TaskStatus) -> TaskStatus {
    original
}

/// OA-07 negative control: a successful retry conceals the original failure.
#[cfg(parking_lot_negative_control = "oa-07")]
fn duplicate_status(_original: TaskStatus, retry: &TaskStatus) -> TaskStatus {
    retry.clone()
}

#[derive(Serialize, Deserialize)]
struct Remembered {
    forget_at_ms: u64,
    status: TaskStatus,
}

#[derive(Deserialize)]
struct LegacyStatusEntry {
    status: TaskStatus,
    idempotency_key: Option<String>,
    expires_at_ms: Option<u64>,
}

/// Only ownership and generation metadata; payloads need not decode to the
/// application's current result type, and their stored bytes are not rewritten.
#[derive(Deserialize)]
struct LegacyEntry {
    idempotency_key: Option<String>,
    expires_at_ms: Option<u64>,
}

fn owner_assignments(
    owners: &[LegacyIdempotencyOwner],
) -> Result<BTreeMap<String, String>, SchedulerError> {
    let mut assignments = BTreeMap::new();
    for owner in owners {
        if assignments
            .insert(owner.idempotency_key.clone(), key_text(&owner.key)?)
            .is_some()
        {
            return Err(SchedulerError::invalid_config(
                "duplicate legacy mailbox idempotency owner assignment",
            ));
        }
    }
    Ok(assignments)
}

/// Convert schema-1 fences in the store's schema transaction. All provenance
/// remains immutable; refusal drops the transaction without changing any table.
pub(crate) fn migrate_idempotency(
    txn: &WriteTransaction,
    now: u64,
    owners: &[LegacyIdempotencyOwner],
) -> Result<(), SchedulerError> {
    let context = "embedded::mailbox::migrate_idempotency";
    let assignments = owner_assignments(owners)?;
    let legacy = txn.open_table(LEGACY_IDEM).map_err(backend(context))?;
    for caller in assignments.keys() {
        let unexpired = legacy
            .get(caller.as_str())
            .map_err(backend(context))?
            .is_some_and(|expiry| expiry.value() > now);
        if !unexpired {
            return Err(SchedulerError::invalid_config(
                "unknown or expired legacy mailbox owner assignment",
            ));
        }
    }
    let entries = txn.open_table(ENTRIES).map_err(backend(context))?;
    let mut candidates: BTreeMap<(String, u64), Vec<String>> = BTreeMap::new();
    for row in entries.iter().map_err(backend(context))? {
        let (key, bytes) = row.map_err(backend(context))?;
        let entry: LegacyEntry = serde_json::from_slice(bytes.value()).map_err(backend(context))?;
        if let Some(caller) = entry.idempotency_key {
            let (mailbox, _) = key.value();
            let address: MailboxKey = serde_json::from_str(mailbox).map_err(backend(context))?;
            candidates
                .entry((caller, entry.expires_at_ms.unwrap_or(u64::MAX)))
                .or_default()
                .push(key_text(&address)?);
        }
    }

    // Multiple retained rows proving the same address are one ownership fact.
    // Status ambiguity is resolved separately, for that same generation.
    for matching in candidates.values_mut() {
        matching.sort();
        matching.dedup();
    }
    let mut scoped = txn.open_table(SCOPED_V2).map_err(backend(context))?;
    let mut provenance = txn.open_table(LEGACY_OWNERS).map_err(backend(context))?;
    for row in legacy.iter().map_err(backend(context))? {
        let (caller, expiry) = row.map_err(backend(context))?;
        let caller = caller.value();
        let expiry = expiry.value();
        if expiry <= now {
            continue;
        }
        let matching = candidates.get(&(caller.to_owned(), expiry));
        let supplied = assignments.get(caller);
        let owner = match (matching.map(Vec::as_slice), supplied) {
            (Some([proven]), None) => proven,
            (Some([proven]), Some(supplied)) if proven == supplied => supplied,
            (Some([_]), Some(_)) => {
                return Err(SchedulerError::invalid_config(
                    "legacy mailbox owner assignment contradicts retained result",
                ));
            }
            (Some(proven), Some(supplied)) if proven.contains(supplied) => supplied,
            (None, Some(supplied)) => supplied,
            _ => {
                return Err(SchedulerError::invalid_config(
                    "legacy mailbox fence needs an authoritative owner assignment",
                ));
            }
        };
        scoped
            .insert((owner.as_str(), caller), expiry)
            .map_err(backend(context))?;
        provenance
            .insert(caller, owner.as_str())
            .map_err(backend(context))?;
    }
    Ok(())
}

/// Validate conversion retries against the immutable resolved owners, even
/// after active fences have expired or been evicted. Never rebuild those fences.
pub(crate) fn validate_idempotency_owners(
    txn: &WriteTransaction,
    owners: &[LegacyIdempotencyOwner],
) -> Result<(), SchedulerError> {
    let context = "embedded::mailbox::validate_idempotency_owners";
    let assignments = owner_assignments(owners)?;
    let provenance = txn.open_table(LEGACY_OWNERS).map_err(backend(context))?;
    for (caller, supplied) in assignments {
        let matches = provenance
            .get(caller.as_str())
            .map_err(backend(context))?
            .is_some_and(|proven| proven.value() == supplied.as_str());
        if !matches {
            return Err(SchedulerError::invalid_config(
                "unknown or conflicting legacy mailbox owner assignment on converted store",
            ));
        }
    }
    Ok(())
}

fn status_assignments(
    statuses: &[LegacyDeliveryStatus],
) -> Result<BTreeMap<(String, String), TaskStatus>, SchedulerError> {
    let mut assignments = BTreeMap::new();
    for evidence in statuses {
        let identity = (key_text(&evidence.key)?, evidence.idempotency_key.clone());
        if assignments
            .insert(identity, evidence.status.clone())
            .is_some()
        {
            return Err(SchedulerError::invalid_config(
                "duplicate legacy mailbox status assignment",
            ));
        }
    }
    Ok(assignments)
}

/// Resolve status from the exact retained generation or authoritative evidence,
/// in the same transaction as any schema-1 ownership conversion and marker.
pub(crate) fn migrate_metadata(
    txn: &WriteTransaction,
    now: u64,
    statuses: &[LegacyDeliveryStatus],
) -> Result<(), SchedulerError> {
    let context = "embedded::mailbox::migrate_metadata";
    let assignments = status_assignments(statuses)?;
    let source = txn.open_table(SCOPED_V2).map_err(backend(context))?;
    for (mailbox, caller) in assignments.keys() {
        let active = source
            .get((mailbox.as_str(), caller.as_str()))
            .map_err(backend(context))?
            .is_some_and(|expiry| expiry.value() > now);
        if !active {
            return Err(SchedulerError::invalid_config(
                "unknown, expired or wrong-owner legacy mailbox status assignment",
            ));
        }
    }
    let entries = txn.open_table(ENTRIES).map_err(backend(context))?;
    let mut candidates: BTreeMap<(String, String, u64), Vec<TaskStatus>> = BTreeMap::new();
    for row in entries.iter().map_err(backend(context))? {
        let (key, bytes) = row.map_err(backend(context))?;
        let entry: LegacyStatusEntry =
            serde_json::from_slice(bytes.value()).map_err(backend(context))?;
        if let Some(caller) = entry.idempotency_key {
            let (mailbox, _) = key.value();
            let address: MailboxKey = serde_json::from_str(mailbox).map_err(backend(context))?;
            let matching = candidates
                .entry((
                    key_text(&address)?,
                    caller,
                    entry.expires_at_ms.unwrap_or(u64::MAX),
                ))
                .or_default();
            if !matching.contains(&entry.status) {
                matching.push(entry.status);
            }
        }
    }
    let mut active = txn.open_table(IDEM).map_err(backend(context))?;
    let mut provenance = txn.open_table(LEGACY_METADATA).map_err(backend(context))?;
    for row in source.iter().map_err(backend(context))? {
        let (key, expiry) = row.map_err(backend(context))?;
        let (mailbox, caller) = key.value();
        let expiry = expiry.value();
        if expiry <= now {
            continue;
        }
        let identity = (mailbox.to_owned(), caller.to_owned());
        let matching = candidates.get(&(mailbox.to_owned(), caller.to_owned(), expiry));
        let supplied = assignments.get(&identity);
        let status = match (matching.map(Vec::as_slice), supplied) {
            (Some([proven]), None) => proven,
            (Some(proven), Some(supplied)) if proven.contains(supplied) => supplied,
            (None, Some(supplied)) => supplied,
            _ => {
                return Err(SchedulerError::invalid_config(
                    "legacy mailbox status is missing, ambiguous or contradicts retained generation",
                ));
            }
        };
        let remembered = serde_json::to_vec(&Remembered {
            forget_at_ms: expiry,
            status: status.clone(),
        })
        .map_err(backend(context))?;
        active
            .insert((mailbox, caller), remembered.as_slice())
            .map_err(backend(context))?;
        provenance
            .insert((mailbox, caller), remembered.as_slice())
            .map_err(backend(context))?;
    }
    Ok(())
}

/// Evidence replay validates immutable metadata; it never reconstructs a fence
/// after consumption, expiry or eviction, or changes a newer generation.
pub(crate) fn validate_metadata(
    txn: &WriteTransaction,
    statuses: &[LegacyDeliveryStatus],
) -> Result<(), SchedulerError> {
    let context = "embedded::mailbox::validate_metadata";
    let assignments = status_assignments(statuses)?;
    let provenance = txn.open_table(LEGACY_METADATA).map_err(backend(context))?;
    for ((mailbox, caller), supplied) in assignments {
        let bytes = provenance
            .get((mailbox.as_str(), caller.as_str()))
            .map_err(backend(context))?
            .ok_or_else(|| {
                SchedulerError::invalid_config(
                    "unknown legacy mailbox status evidence on converted store",
                )
            })?;
        let proven: Remembered = serde_json::from_slice(bytes.value()).map_err(backend(context))?;
        if proven.status != supplied {
            return Err(SchedulerError::invalid_config(
                "conflicting legacy mailbox status evidence on converted store",
            ));
        }
    }
    Ok(())
}

/// A crash-safe [`ResultMailbox`] stored in a redb file.
///
/// Created with [`EmbeddedStore::result_mailbox`]. Payloads are stored as JSON,
/// so `T` must implement `Serialize` and `DeserializeOwned`.
#[derive(Debug)]
pub struct RedbResultMailbox<T> {
    store: EmbeddedStore,
    payload: PhantomData<fn() -> T>,
}

impl<T> RedbResultMailbox<T> {
    pub(crate) const fn new(store: EmbeddedStore) -> Self {
        Self {
            store,
            payload: PhantomData,
        }
    }
}

fn next_sequence(meta: &mut Table<'_, &str, u64>) -> Result<u64, SchedulerError> {
    let context = "embedded::mailbox::sequence";
    let current = meta
        .get(NEXT_SEQUENCE)
        .map_err(backend(context))?
        .map_or(1, |guard| guard.value());
    meta.insert(NEXT_SEQUENCE, current + 1)
        .map_err(backend(context))?;
    Ok(current)
}

fn deliver_tx<T: Serialize>(
    db: &redb::Database,
    now: u64,
    delivery: Delivery<T>,
) -> Result<DeliveryOutcome, SchedulerError> {
    let context = "embedded::mailbox::deliver";
    let key = key_text(&delivery.key)?;
    let scope = idempotency_address(&key);
    let expires_at_ms = delivery.ttl.map(|ttl| now.saturating_add(duration_ms(ttl)));
    let idempotency_key = delivery.idempotency_key.clone();
    let stored = Stored {
        task_id: delivery.task_id,
        status: delivery.status,
        payload: delivery.payload,
        idempotency_key: delivery.idempotency_key,
        delivered_at_ms: u128::from(now),
        expires_at_ms,
    };
    let bytes = serde_json::to_vec(&stored).map_err(backend(context))?;

    let txn = db.begin_write().map_err(backend(context))?;
    {
        let mut entries = txn.open_table(ENTRIES).map_err(backend(context))?;
        let mut idem = txn.open_table(IDEM).map_err(backend(context))?;
        let mut meta = txn.open_table(META).map_err(backend(context))?;
        if let Some(idem_key) = &idempotency_key {
            let remembered = idem
                .get((scope, idem_key.as_str()))
                .map_err(backend(context))?
                .map(|bytes| serde_json::from_slice::<Remembered>(bytes.value()))
                .transpose()
                .map_err(backend(context))?;
            if let Some(remembered) = remembered.filter(|record| record.forget_at_ms > now) {
                return Ok(DeliveryOutcome::Duplicate {
                    status: duplicate_status(remembered.status, &stored.status),
                });
            }
            let remembered = serde_json::to_vec(&Remembered {
                forget_at_ms: expires_at_ms.unwrap_or(u64::MAX),
                status: stored.status,
            })
            .map_err(backend(context))?;
            idem.insert((scope, idem_key.as_str()), remembered.as_slice())
                .map_err(backend(context))?;
        }
        let sequence = next_sequence(&mut meta)?;
        entries
            .insert((key.as_str(), sequence), bytes.as_slice())
            .map_err(backend(context))?;
    }
    commit(txn, "deliver")?;
    Ok(DeliveryOutcome::Stored)
}

fn consume_tx<T: DeserializeOwned>(
    db: &redb::Database,
    now: u64,
    key: &MailboxKey,
) -> Result<Option<MailboxEntry<T>>, SchedulerError> {
    let context = "embedded::mailbox::consume";
    let key = key_text(key)?;
    let txn = db.begin_write().map_err(backend(context))?;
    let (taken, changed, decode_error) = {
        let mut entries = txn.open_table(ENTRIES).map_err(backend(context))?;
        let mut remove = Vec::new();
        let mut taken = None;
        let mut decode_error = None;
        for item in entries
            .range::<(&str, u64)>((key.as_str(), 0)..=(key.as_str(), u64::MAX))
            .map_err(backend(context))?
        {
            let (entry_key, bytes) = item.map_err(backend(context))?;
            let (_, sequence) = entry_key.value();
            remove.push(sequence);
            if !is_expired(expiry_of(bytes.value())?, now) {
                // An entry that no longer decodes (a type change between
                // deploys) is removed with the rest, like the Postgres
                // mailbox, so the error is reported once, not on every call.
                match serde_json::from_slice::<Stored<T>>(bytes.value()) {
                    Ok(stored) => taken = Some(stored),
                    Err(error) => decode_error = Some(error),
                }
                break;
            }
        }
        for sequence in &remove {
            entries
                .remove((key.as_str(), *sequence))
                .map_err(backend(context))?;
        }
        (taken, !remove.is_empty(), decode_error)
    };
    if changed {
        commit(txn, "consume")?;
    }
    if let Some(error) = decode_error {
        return Err(backend(context)(error));
    }
    Ok(taken.map(Stored::into_entry))
}

fn peek_tx<T: DeserializeOwned>(
    db: &redb::Database,
    now: u64,
    key: &MailboxKey,
) -> Result<Option<MailboxEntry<T>>, SchedulerError> {
    let context = "embedded::mailbox::peek";
    let key = key_text(key)?;
    let txn = db.begin_read().map_err(backend(context))?;
    let entries = txn.open_table(ENTRIES).map_err(backend(context))?;
    for item in entries
        .range::<(&str, u64)>((key.as_str(), 0)..=(key.as_str(), u64::MAX))
        .map_err(backend(context))?
    {
        let (_, bytes) = item.map_err(backend(context))?;
        if !is_expired(expiry_of(bytes.value())?, now) {
            let stored: Stored<T> =
                serde_json::from_slice(bytes.value()).map_err(backend(context))?;
            return Ok(Some(stored.into_entry()));
        }
    }
    Ok(None)
}

fn evict_tx(db: &redb::Database, now: u64) -> Result<usize, SchedulerError> {
    let context = "embedded::mailbox::evict";
    let txn = db.begin_write().map_err(backend(context))?;
    let evicted = {
        let mut entries = txn.open_table(ENTRIES).map_err(backend(context))?;
        let mut idem = txn.open_table(IDEM).map_err(backend(context))?;

        let mut expired_entries = Vec::new();
        for item in entries.iter().map_err(backend(context))? {
            let (key, bytes) = item.map_err(backend(context))?;
            if is_expired(expiry_of(bytes.value())?, now) {
                let (mailbox, sequence) = key.value();
                expired_entries.push((mailbox.to_owned(), sequence));
            }
        }
        for (mailbox, sequence) in &expired_entries {
            entries
                .remove((mailbox.as_str(), *sequence))
                .map_err(backend(context))?;
        }

        let mut forgotten = Vec::new();
        for item in idem.iter().map_err(backend(context))? {
            let (key, bytes) = item.map_err(backend(context))?;
            let remembered: Remembered =
                serde_json::from_slice(bytes.value()).map_err(backend(context))?;
            if remembered.forget_at_ms <= now {
                let (mailbox, caller) = key.value();
                forgotten.push((mailbox.to_owned(), caller.to_owned()));
            }
        }
        for (mailbox, caller) in &forgotten {
            idem.remove((mailbox.as_str(), caller.as_str()))
                .map_err(backend(context))?;
        }
        expired_entries.len()
    };
    commit(txn, "evict")?;
    Ok(evicted)
}

impl<T> ResultMailbox<T> for RedbResultMailbox<T>
where
    T: Serialize + DeserializeOwned + Send + 'static,
{
    fn deliver(
        &self,
        delivery: Delivery<T>,
    ) -> impl Future<Output = Result<DeliveryOutcome, SchedulerError>> + Send {
        self.store
            .blocking(move |db, clock| deliver_tx(db, now_ms(clock), delivery))
    }

    fn consume(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send {
        let key = key.clone();
        self.store
            .blocking(move |db, clock| consume_tx::<T>(db, now_ms(clock), &key))
    }

    fn peek(
        &self,
        key: &MailboxKey,
    ) -> impl Future<Output = Result<Option<MailboxEntry<T>>, SchedulerError>> + Send {
        let key = key.clone();
        self.store
            .blocking(move |db, clock| peek_tx::<T>(db, now_ms(clock), &key))
    }

    fn evict_expired(&self) -> impl Future<Output = Result<usize, SchedulerError>> + Send {
        self.store.blocking(|db, clock| evict_tx(db, now_ms(clock)))
    }
}
