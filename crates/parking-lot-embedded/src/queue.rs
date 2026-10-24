//! [`RedbLeaseQueue`]: a crash-safe [`LeaseQueue`] on redb (ADR 0004).
//!
//! # Layout
//!
//! One redb table per index; every operation is one write transaction over
//! them, so a crash leaves each operation either fully applied or not at all.
//!
//! | table | key | value | holds |
//! |---|---|---|---|
//! | `q_tasks` | entry id | task JSON | every live task, immutable after enqueue |
//! | `q_state` | entry id | state row (kind, priority, time, fence, attempts) | lifecycle state, fence, attempts |
//! | `q_avail` | (inverted priority, entry id) | | entries a lease may take now |
//! | `q_delayed` | (not-before ms, entry id) | | requeued with a delay |
//! | `q_leased` | (lease expiry ms, entry id) | | entries under a lease |
//! | `q_idem` / `q_idem_rev` | idempotency key / entry id | entry id / key | live keys |
//! | `q_dead` | entry id | dead-letter JSON | given-up tasks |
//! | `q_meta` | name | counter | next entry id, next fence, layout version, store id (the queue's identity) |
//!
//! Entry ids come from a counter that only grows, so id order is enqueue order
//! and the `q_avail` key `(255 - priority, id)` is exactly "highest priority
//! first, then FIFO". A lease first moves expired leases and due delays back
//! into `q_avail`, then pops from its front.

use std::future::Future;
use std::marker::PhantomData;
use std::time::Duration;

use prometheus_parking_lot_core::{
    Clock, Enqueued, LeaseQueue, LeaseToken, LeasedTask, NackAction, Priority, QueueDepth,
    QueueEntryId, ScheduledTask, SchedulerError,
};
use redb::{
    ReadTransaction, ReadableDatabase, ReadableTable, ReadableTableMetadata, Table,
    TableDefinition, WriteTransaction,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::store::{EmbeddedStore, backend, commit, duration_ms, now_ms};

/// Name reported in [`SchedulerError::QueueFull`].
const QUEUE_NAME: &str = "embedded-redb";

const NEXT_ID: &str = "next_id";
const NEXT_FENCE: &str = "next_fence";

type StateValue = (u8, u8, u64, u64, u32);

const TASKS: TableDefinition<u64, &[u8]> = TableDefinition::new("q_tasks");
const STATE: TableDefinition<u64, StateValue> = TableDefinition::new("q_state");
const AVAIL: TableDefinition<(u8, u64), ()> = TableDefinition::new("q_avail");
const DELAYED: TableDefinition<(u64, u64), ()> = TableDefinition::new("q_delayed");
const LEASED: TableDefinition<(u64, u64), ()> = TableDefinition::new("q_leased");
const IDEM: TableDefinition<&str, u64> = TableDefinition::new("q_idem");
const IDEM_REV: TableDefinition<u64, &str> = TableDefinition::new("q_idem_rev");
const DEAD: TableDefinition<u64, &[u8]> = TableDefinition::new("q_dead");
const META: TableDefinition<&str, u64> = TableDefinition::new("q_meta");

/// Where an entry is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// In `q_avail`.
    Ready,
    /// In `q_delayed` until `time_ms`.
    Delayed,
    /// In `q_leased` until `time_ms`.
    Leased,
}

impl Kind {
    const fn code(self) -> u8 {
        match self {
            Self::Ready => 0,
            Self::Delayed => 1,
            Self::Leased => 2,
        }
    }

    fn from_code(code: u8) -> Result<Self, SchedulerError> {
        match code {
            0 => Ok(Self::Ready),
            1 => Ok(Self::Delayed),
            2 => Ok(Self::Leased),
            other => Err(SchedulerError::internal(format!(
                "embedded queue state row has unknown kind {other}"
            ))),
        }
    }
}

/// The small, frequently rewritten part of an entry.
#[derive(Debug, Clone, Copy)]
struct StateRow {
    kind: Kind,
    /// `255 - priority rank`: ascending key order is descending priority.
    inverted_priority: u8,
    /// Not-before time (`Delayed`) or lease expiry (`Leased`), in wall ms.
    time_ms: u64,
    /// Fence of the current or most recent lease; 0 before the first lease.
    fence: u64,
    /// Times the entry has been leased.
    attempts: u32,
}

impl StateRow {
    fn decode(value: StateValue) -> Result<Self, SchedulerError> {
        let (kind, inverted_priority, time_ms, fence, attempts) = value;
        Ok(Self {
            kind: Kind::from_code(kind)?,
            inverted_priority,
            time_ms,
            fence,
            attempts,
        })
    }

    const fn encode(self) -> StateValue {
        (
            self.kind.code(),
            self.inverted_priority,
            self.time_ms,
            self.fence,
            self.attempts,
        )
    }
}

/// `255 - rank`, so that ascending order is highest priority first.
const fn inverted_priority(priority: Priority) -> u8 {
    let rank = match priority {
        Priority::Low => 0,
        Priority::Normal => 1,
        Priority::High => 2,
        Priority::Critical => 3,
    };
    u8::MAX - rank
}

/// Create every queue table and the metadata table in `txn`.
pub(crate) fn create_tables(txn: &WriteTransaction) -> Result<(), SchedulerError> {
    let context = "embedded::queue::create_tables";
    txn.open_table(TASKS).map_err(backend(context))?;
    txn.open_table(STATE).map_err(backend(context))?;
    txn.open_table(AVAIL).map_err(backend(context))?;
    txn.open_table(DELAYED).map_err(backend(context))?;
    txn.open_table(LEASED).map_err(backend(context))?;
    txn.open_table(IDEM).map_err(backend(context))?;
    txn.open_table(IDEM_REV).map_err(backend(context))?;
    txn.open_table(DEAD).map_err(backend(context))?;
    txn.open_table(META).map_err(backend(context))?;
    Ok(())
}

/// Read a metadata counter inside `txn`.
pub(crate) fn meta_get(txn: &WriteTransaction, name: &str) -> Result<Option<u64>, SchedulerError> {
    let meta = txn
        .open_table(META)
        .map_err(backend("embedded::queue::meta"))?;
    let value = meta
        .get(name)
        .map_err(backend("embedded::queue::meta_get"))?
        .map(|guard| guard.value());
    Ok(value)
}

/// Write a metadata counter inside `txn`.
pub(crate) fn meta_set(
    txn: &WriteTransaction,
    name: &str,
    value: u64,
) -> Result<(), SchedulerError> {
    let mut meta = txn
        .open_table(META)
        .map_err(backend("embedded::queue::meta"))?;
    meta.insert(name, value)
        .map_err(backend("embedded::queue::meta_set"))?;
    Ok(())
}

/// All queue tables, open inside one write transaction.
struct Tables<'txn> {
    tasks: Table<'txn, u64, &'static [u8]>,
    state: Table<'txn, u64, StateValue>,
    avail: Table<'txn, (u8, u64), ()>,
    delayed: Table<'txn, (u64, u64), ()>,
    leased: Table<'txn, (u64, u64), ()>,
    idem: Table<'txn, &'static str, u64>,
    idem_rev: Table<'txn, u64, &'static str>,
    dead: Table<'txn, u64, &'static [u8]>,
    meta: Table<'txn, &'static str, u64>,
}

impl<'txn> Tables<'txn> {
    fn open(txn: &'txn WriteTransaction) -> Result<Self, SchedulerError> {
        let context = "embedded::queue::open_tables";
        Ok(Self {
            tasks: txn.open_table(TASKS).map_err(backend(context))?,
            state: txn.open_table(STATE).map_err(backend(context))?,
            avail: txn.open_table(AVAIL).map_err(backend(context))?,
            delayed: txn.open_table(DELAYED).map_err(backend(context))?,
            leased: txn.open_table(LEASED).map_err(backend(context))?,
            idem: txn.open_table(IDEM).map_err(backend(context))?,
            idem_rev: txn.open_table(IDEM_REV).map_err(backend(context))?,
            dead: txn.open_table(DEAD).map_err(backend(context))?,
            meta: txn.open_table(META).map_err(backend(context))?,
        })
    }

    /// Take the next value of a counter that starts at 1.
    fn next(&mut self, name: &str) -> Result<u64, SchedulerError> {
        let context = "embedded::queue::counter";
        let current = self
            .meta
            .get(name)
            .map_err(backend(context))?
            .map_or(1, |guard| guard.value());
        self.meta
            .insert(name, current + 1)
            .map_err(backend(context))?;
        Ok(current)
    }

    fn state_of(&self, id: u64) -> Result<Option<StateRow>, SchedulerError> {
        let raw = self
            .state
            .get(id)
            .map_err(backend("embedded::queue::state_get"))?
            .map(|guard| guard.value());
        raw.map(StateRow::decode).transpose()
    }

    fn set_state(&mut self, id: u64, row: StateRow) -> Result<(), SchedulerError> {
        self.state
            .insert(id, row.encode())
            .map_err(backend("embedded::queue::state_set"))?;
        Ok(())
    }

    /// The row of the lease `token` names, if that lease is current and
    /// unexpired at `now`; otherwise `LeaseLost`.
    fn held(&self, token: LeaseToken, now: u64) -> Result<StateRow, SchedulerError> {
        match self.state_of(token.entry().0)? {
            Some(row)
                if row.kind == Kind::Leased && row.fence == token.fence() && now < row.time_ms =>
            {
                Ok(row)
            }
            _ => Err(token.lost()),
        }
    }

    /// Put `id` back in the ready set with `row`'s priority.
    fn make_ready(&mut self, id: u64, mut row: StateRow) -> Result<(), SchedulerError> {
        row.kind = Kind::Ready;
        row.time_ms = 0;
        self.avail
            .insert((row.inverted_priority, id), ())
            .map_err(backend("embedded::queue::avail_insert"))?;
        self.set_state(id, row)
    }

    /// Forget a live entry's idempotency key.
    fn release_key(&mut self, id: u64) -> Result<(), SchedulerError> {
        let context = "embedded::queue::idem_release";
        let key = self
            .idem_rev
            .remove(id)
            .map_err(backend(context))?
            .map(|guard| guard.value().to_owned());
        if let Some(key) = key {
            self.idem.remove(key.as_str()).map_err(backend(context))?;
        }
        Ok(())
    }

    /// Move every lease that expired at or before `now`, and every delay that
    /// elapsed, back to the ready set. Returns whether anything moved.
    fn promote_due(&mut self, now: u64) -> Result<bool, SchedulerError> {
        let context = "embedded::queue::promote";
        let expired = collect_due(&self.leased, now)?;
        for &(until, id) in &expired {
            self.leased.remove((until, id)).map_err(backend(context))?;
            if let Some(row) = self.state_of(id)? {
                self.make_ready(id, row)?;
            }
        }
        let due = collect_due(&self.delayed, now)?;
        for &(not_before, id) in &due {
            self.delayed
                .remove((not_before, id))
                .map_err(backend(context))?;
            if let Some(row) = self.state_of(id)? {
                self.make_ready(id, row)?;
            }
        }
        Ok(!expired.is_empty() || !due.is_empty())
    }

    /// Move a live entry that is in no index to the dead letters, keeping its
    /// task JSON verbatim (or as a string when it is not valid JSON), and free
    /// its idempotency key.
    fn bury(&mut self, id: u64, attempts: u32, reason: &str) -> Result<(), SchedulerError> {
        let context = "embedded::queue::dead_letter";
        let task_json = self
            .tasks
            .remove(id)
            .map_err(backend(context))?
            .map(|guard| guard.value().to_vec())
            .unwrap_or_default();
        let task = serde_json::from_slice::<serde_json::Value>(&task_json).unwrap_or_else(|_| {
            serde_json::Value::String(String::from_utf8_lossy(&task_json).into_owned())
        });
        let letter = serde_json::json!({
            "task": task,
            "attempts": attempts,
            "reason": reason,
        });
        let letter = serde_json::to_vec(&letter).map_err(backend(context))?;
        self.dead
            .insert(id, letter.as_slice())
            .map_err(backend(context))?;
        self.state.remove(id).map_err(backend(context))?;
        self.release_key(id)
    }

    /// The stored task, or the decode error when its JSON is not a `P`.
    fn decode_task<P: DeserializeOwned>(
        &self,
        id: u64,
    ) -> Result<Result<ScheduledTask<P>, serde_json::Error>, SchedulerError> {
        let guard = self
            .tasks
            .get(id)
            .map_err(backend("embedded::queue::task_get"))?
            .ok_or_else(|| {
                SchedulerError::internal(format!("embedded queue entry {id} has no task row"))
            })?;
        Ok(serde_json::from_slice(guard.value()))
    }
}

/// `(time, id)` keys of `table` whose time is at or before `now`.
fn collect_due(
    table: &impl ReadableTable<(u64, u64), ()>,
    now: u64,
) -> Result<Vec<(u64, u64)>, SchedulerError> {
    let context = "embedded::queue::collect_due";
    let mut due = Vec::new();
    for item in table
        .range::<(u64, u64)>(..=(now, u64::MAX))
        .map_err(backend(context))?
    {
        let (key, _) = item.map_err(backend(context))?;
        due.push(key.value());
    }
    Ok(due)
}

/// Entries of a `(time, id)` index with time at or before `now`.
fn count_due(
    table: &impl ReadableTable<(u64, u64), ()>,
    now: u64,
) -> Result<usize, SchedulerError> {
    let context = "embedded::queue::count_due";
    let mut due = 0;
    for item in table
        .range::<(u64, u64)>(..=(now, u64::MAX))
        .map_err(backend(context))?
    {
        item.map_err(backend(context))?;
        due += 1;
    }
    Ok(due)
}

fn require_positive(lease_for: Duration) -> Result<(), SchedulerError> {
    if lease_for.is_zero() {
        return Err(SchedulerError::invalid_config(
            "lease duration must be greater than zero",
        ));
    }
    Ok(())
}

fn len_of(table: &impl ReadableTableMetadata) -> Result<usize, SchedulerError> {
    let len = table.len().map_err(backend("embedded::queue::len"))?;
    Ok(usize::try_from(len).unwrap_or(usize::MAX))
}

/// A crash-safe [`LeaseQueue`] stored in a redb file.
///
/// Created with [`EmbeddedStore::lease_queue`]. Payloads are stored as JSON, so
/// `P` must implement `Serialize` and `DeserializeOwned`. See the
/// [crate documentation](crate) for the durability guarantee and the module
/// documentation for the storage layout.
///
/// Operations are cancellation-safe in the sense the [`LeaseQueue`] contract
/// requires: dropping a future never abandons a transaction half done; the
/// blocking work finishes and the operation either applied or did not.
#[derive(Debug)]
pub struct RedbLeaseQueue<P> {
    store: EmbeddedStore,
    max_depth: usize,
    max_attempts: Option<u32>,
    payload: PhantomData<fn() -> P>,
}

impl<P> RedbLeaseQueue<P> {
    pub(crate) const fn new(store: EmbeddedStore, max_depth: usize) -> Self {
        Self {
            store,
            max_depth,
            max_attempts: None,
            payload: PhantomData,
        }
    }

    /// Dead-letter an entry that has been leased `max_attempts` times instead
    /// of leasing it again: a task is delivered exactly `max_attempts` times,
    /// then the next lease moves it to the dead letters with the reason
    /// `max attempts (N) exhausted`. Unset by default (redeliver forever).
    ///
    /// Without a limit, a task that kills its process is redelivered on every
    /// restart; a desktop app that crashes on it crashes again on every launch.
    #[must_use]
    pub const fn with_max_attempts(mut self, max_attempts: u32) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }

    /// The dead letters, oldest first, as `(entry id, JSON)` where the JSON
    /// holds `task`, `attempts` and `reason`.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the database cannot be read.
    pub async fn dead_letters(&self) -> Result<Vec<(QueueEntryId, String)>, SchedulerError> {
        self.store
            .blocking(|db, _| {
                let context = "embedded::queue::dead_letters";
                let txn = db.begin_read().map_err(backend(context))?;
                let dead = txn.open_table(DEAD).map_err(backend(context))?;
                let mut letters = Vec::new();
                for item in dead.iter().map_err(backend(context))? {
                    let (id, json) = item.map_err(backend(context))?;
                    let json = String::from_utf8_lossy(json.value()).into_owned();
                    letters.push((QueueEntryId(id.value()), json));
                }
                Ok(letters)
            })
            .await
    }
}

/// A storage failure raised before the enqueue commits stored nothing: report
/// it as [`SchedulerError::NotStored`] so callers can tell it from an unknown
/// outcome. Other errors (`QueueFull`, …) pass through unchanged.
fn not_stored(error: SchedulerError) -> SchedulerError {
    match error {
        SchedulerError::Backend { context, source } => {
            SchedulerError::NotStored { context, source }
        }
        other => other,
    }
}

fn enqueue_tx(
    db: &redb::Database,
    max_depth: usize,
    task_json: &[u8],
    priority: Priority,
    key: Option<&str>,
) -> Result<Enqueued, SchedulerError> {
    let txn = db
        .begin_write()
        .map_err(backend("embedded::queue::enqueue::begin"))
        .map_err(not_stored)?;
    // Everything up to `commit` writes only into the uncommitted transaction;
    // an error drops it and nothing is stored.
    let staged = stage_enqueue(&txn, max_depth, task_json, priority, key).map_err(not_stored)?;
    match staged {
        Enqueued::Accepted(id) => {
            commit(txn, "enqueue")?;
            Ok(Enqueued::Accepted(id))
        }
        duplicate @ Enqueued::Duplicate(_) => Ok(duplicate),
    }
}

fn stage_enqueue(
    txn: &redb::WriteTransaction,
    max_depth: usize,
    task_json: &[u8],
    priority: Priority,
    key: Option<&str>,
) -> Result<Enqueued, SchedulerError> {
    let accepted = {
        let mut t = Tables::open(txn)?;
        if let Some(key) = key
            && let Some(existing) = t
                .idem
                .get(key)
                .map_err(backend("embedded::queue::enqueue::idem"))?
        {
            return Ok(Enqueued::Duplicate(QueueEntryId(existing.value())));
        }
        let live = len_of(&t.tasks)?;
        if live >= max_depth {
            return Err(SchedulerError::queue_full(QUEUE_NAME, live, max_depth));
        }
        let id = t.next(NEXT_ID)?;
        let context = "embedded::queue::enqueue::insert";
        t.tasks.insert(id, task_json).map_err(backend(context))?;
        t.make_ready(
            id,
            StateRow {
                kind: Kind::Ready,
                inverted_priority: inverted_priority(priority),
                time_ms: 0,
                fence: 0,
                attempts: 0,
            },
        )?;
        if let Some(key) = key {
            t.idem.insert(key, id).map_err(backend(context))?;
            t.idem_rev.insert(id, key).map_err(backend(context))?;
        }
        id
    };
    Ok(Enqueued::Accepted(QueueEntryId(accepted)))
}

fn lease_tx<P: DeserializeOwned>(
    db: &redb::Database,
    clock: &dyn Clock,
    max: usize,
    lease_for: Duration,
    max_attempts: Option<u32>,
) -> Result<Vec<LeasedTask<P>>, SchedulerError> {
    let context = "embedded::queue::lease";
    let now = now_ms(clock);
    let until = now.saturating_add(duration_ms(lease_for));
    let txn = db.begin_write().map_err(backend(context))?;
    let mut leased = Vec::new();
    let mut changed;
    {
        let mut t = Tables::open(&txn)?;
        changed = t.promote_due(now)?;
        while leased.len() < max {
            let Some(front) = t.avail.pop_first().map_err(backend(context))? else {
                break;
            };
            let (_, id) = front.0.value();
            drop(front);
            changed = true;
            let mut row = t.state_of(id)?.ok_or_else(|| {
                SchedulerError::internal(format!("embedded queue entry {id} has no state row"))
            })?;
            if let Some(limit) = max_attempts
                && row.attempts >= limit
            {
                t.bury(
                    id,
                    row.attempts,
                    &format!("max attempts ({limit}) exhausted"),
                )?;
                continue;
            }
            // An entry this build cannot decode (for example after an update
            // changed the payload type) is dead-lettered in this transaction
            // instead of failing the lease, which would roll back the
            // transaction and leave the entry at the front for every later
            // lease.
            let task = match t.decode_task::<P>(id)? {
                Ok(task) => task,
                Err(error) => {
                    t.bury(id, row.attempts, &format!("undecodable task: {error}"))?;
                    continue;
                }
            };
            row.attempts = row.attempts.saturating_add(1);
            row.fence = t.next(NEXT_FENCE)?;
            row.kind = Kind::Leased;
            row.time_ms = until;
            t.leased.insert((until, id), ()).map_err(backend(context))?;
            t.set_state(id, row)?;
            leased.push(LeasedTask::new(
                task,
                LeaseToken::new(QueueEntryId(id), row.fence),
                row.attempts,
            ));
        }
    }
    if changed {
        commit(txn, "lease")?;
    }
    Ok(leased)
}

fn ack_tx(db: &redb::Database, clock: &dyn Clock, token: LeaseToken) -> Result<(), SchedulerError> {
    let context = "embedded::queue::ack";
    let now = now_ms(clock);
    let id = token.entry().0;
    let txn = db.begin_write().map_err(backend(context))?;
    {
        let mut t = Tables::open(&txn)?;
        let row = t.held(token, now)?;
        t.leased
            .remove((row.time_ms, id))
            .map_err(backend(context))?;
        t.state.remove(id).map_err(backend(context))?;
        t.tasks.remove(id).map_err(backend(context))?;
        t.release_key(id)?;
    }
    commit(txn, "ack")
}

fn nack_tx(
    db: &redb::Database,
    clock: &dyn Clock,
    token: LeaseToken,
    action: &NackAction,
) -> Result<(), SchedulerError> {
    let context = "embedded::queue::nack";
    let now = now_ms(clock);
    let id = token.entry().0;
    let txn = db.begin_write().map_err(backend(context))?;
    {
        let mut t = Tables::open(&txn)?;
        let mut row = t.held(token, now)?;
        t.leased
            .remove((row.time_ms, id))
            .map_err(backend(context))?;
        match action {
            NackAction::Requeue { delay } if delay.is_zero() => t.make_ready(id, row)?,
            NackAction::Requeue { delay } => {
                let not_before = now.saturating_add(duration_ms(*delay));
                row.kind = Kind::Delayed;
                row.time_ms = not_before;
                t.delayed
                    .insert((not_before, id), ())
                    .map_err(backend(context))?;
                t.set_state(id, row)?;
            }
            NackAction::DeadLetter { reason } => t.bury(id, row.attempts, reason)?,
            // `NackAction` is non-exhaustive: refuse actions this build cannot honour.
            _ => {
                return Err(SchedulerError::invalid_config(
                    "unsupported nack action for the embedded queue",
                ));
            }
        }
    }
    commit(txn, "nack")
}

fn extend_tx(
    db: &redb::Database,
    clock: &dyn Clock,
    token: LeaseToken,
    lease_for: Duration,
) -> Result<(), SchedulerError> {
    let context = "embedded::queue::extend";
    let now = now_ms(clock);
    let until = now.saturating_add(duration_ms(lease_for));
    let id = token.entry().0;
    let txn = db.begin_write().map_err(backend(context))?;
    {
        let mut t = Tables::open(&txn)?;
        let mut row = t.held(token, now)?;
        t.leased
            .remove((row.time_ms, id))
            .map_err(backend(context))?;
        t.leased.insert((until, id), ()).map_err(backend(context))?;
        row.time_ms = until;
        t.set_state(id, row)?;
    }
    commit(txn, "extend")
}

fn depth_tx(db: &redb::Database, clock: &dyn Clock) -> Result<QueueDepth, SchedulerError> {
    let context = "embedded::queue::depth";
    let now = now_ms(clock);
    let txn: ReadTransaction = db.begin_read().map_err(backend(context))?;
    let avail = txn.open_table(AVAIL).map_err(backend(context))?;
    let delayed = txn.open_table(DELAYED).map_err(backend(context))?;
    let leased = txn.open_table(LEASED).map_err(backend(context))?;
    let dead = txn.open_table(DEAD).map_err(backend(context))?;

    let expired = count_due(&leased, now)?;
    let due = count_due(&delayed, now)?;
    Ok(QueueDepth::new(
        len_of(&avail)? + expired + due,
        len_of(&delayed)? - due,
        len_of(&leased)? - expired,
        len_of(&dead)?,
    ))
}

impl<P> LeaseQueue<P> for RedbLeaseQueue<P>
where
    P: Serialize + DeserializeOwned + Send + 'static,
{
    fn enqueue(
        &self,
        task: ScheduledTask<P>,
    ) -> impl Future<Output = Result<Enqueued, SchedulerError>> + Send {
        let max_depth = self.max_depth;
        self.store.blocking(move |db, _| {
            let task_json = serde_json::to_vec(&task)
                .map_err(backend("embedded::queue::encode"))
                .map_err(not_stored)?;
            enqueue_tx(
                db,
                max_depth,
                &task_json,
                task.meta.priority,
                task.meta.idempotency_key.as_deref(),
            )
        })
    }

    fn lease(
        &self,
        max: usize,
        lease_for: Duration,
    ) -> impl Future<Output = Result<Vec<LeasedTask<P>>, SchedulerError>> + Send {
        let validated = require_positive(lease_for);
        let store = self.store.clone();
        let max_attempts = self.max_attempts;
        async move {
            validated?;
            if max == 0 {
                return Ok(Vec::new());
            }
            store
                .blocking(move |db, clock| lease_tx::<P>(db, clock, max, lease_for, max_attempts))
                .await
        }
    }

    fn ack(&self, token: LeaseToken) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.store
            .blocking(move |db, clock| ack_tx(db, clock, token))
    }

    fn nack(
        &self,
        token: LeaseToken,
        action: NackAction,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        self.store
            .blocking(move |db, clock| nack_tx(db, clock, token, &action))
    }

    fn extend(
        &self,
        token: LeaseToken,
        lease_for: Duration,
    ) -> impl Future<Output = Result<(), SchedulerError>> + Send {
        let validated = require_positive(lease_for);
        let store = self.store.clone();
        async move {
            validated?;
            store
                .blocking(move |db, clock| extend_tx(db, clock, token, lease_for))
                .await
        }
    }

    fn depth(&self) -> impl Future<Output = Result<QueueDepth, SchedulerError>> + Send {
        self.store.blocking(depth_tx)
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    /// `redb:<store id>`: a random id generated when the database file is
    /// first opened and kept in it, so it survives restarts and moving the
    /// file, and a recreated file gets a new one.
    fn identity(&self) -> &str {
        &self.store.identity
    }
}
