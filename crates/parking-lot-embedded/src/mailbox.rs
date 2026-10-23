//! [`RedbResultMailbox`]: a crash-safe [`ResultMailbox`] on redb (ADR 0004).
//!
//! # Layout
//!
//! | table | key | value | holds |
//! |---|---|---|---|
//! | `mb_entries` | (mailbox key JSON, sequence) | entry JSON | undelivered results, in delivery order |
//! | `mb_idem` | idempotency key | forget-at ms (`u64::MAX`: never) | remembered keys, kept past consume |
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

use std::future::Future;
use std::marker::PhantomData;

use prometheus_parking_lot_core::{
    Delivery, DeliveryOutcome, MailboxEntry, MailboxKey, ResultMailbox, SchedulerError, TaskId,
    TaskStatus,
};
use redb::{ReadableDatabase, ReadableTable, Table, TableDefinition, WriteTransaction};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::store::{EmbeddedStore, backend, commit, duration_ms, now_ms};

const NEXT_SEQUENCE: &str = "next_sequence";

const ENTRIES: TableDefinition<(&str, u64), &[u8]> = TableDefinition::new("mb_entries");
const IDEM: TableDefinition<&str, u64> = TableDefinition::new("mb_idem");
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
    txn.open_table(META).map_err(backend(context))?;
    Ok(())
}

/// The canonical text form of a mailbox key (its JSON).
fn key_text(key: &MailboxKey) -> Result<String, SchedulerError> {
    serde_json::to_string(key).map_err(backend("embedded::mailbox::encode_key"))
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
                .get(idem_key.as_str())
                .map_err(backend(context))?
                .is_some_and(|forget_at| forget_at.value() > now);
            if remembered {
                return Ok(DeliveryOutcome::Duplicate);
            }
            idem.insert(idem_key.as_str(), expires_at_ms.unwrap_or(u64::MAX))
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
            let (key, forget_at) = item.map_err(backend(context))?;
            if forget_at.value() <= now {
                forgotten.push(key.value().to_owned());
            }
        }
        for key in &forgotten {
            idem.remove(key.as_str()).map_err(backend(context))?;
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
