//! Postgres audit sink: [`AuditEvent`]s written to `pl_audit_events`.
//!
//! [`AuditSink::record`] is synchronous and must not block a scheduler thread
//! on the network, so the sink only queues the event on a bounded channel; a
//! background task inserts queued events in batches. Three consequences:
//!
//! - **`record` never blocks and never fails.** When the channel is full (the
//!   database is slower than the producers) the event is dropped, counted and
//!   logged at `warn`; [`PostgresAuditSink::lost`] reports the count. An audit
//!   log must not stall the work it describes.
//! - **Events are written shortly after `record`, not during it.** Call
//!   [`shutdown`](PostgresAuditSink::shutdown) before exit to wait for the
//!   queue to drain.
//! - **A failed write is lost, counted and logged at `error`.** There is no
//!   retry of the same write; an audit sink that retried forever would hold
//!   events (and memory) during an outage. If the database rejects a batch (one
//!   event it cannot store, such as a NUL byte), its events are written one at a
//!   time so only the rejected ones are lost; a connection or pool failure loses
//!   the whole batch. For a replayable event log use the Iggy events backend.
//!
//! The write is idempotent: `event_id` is the primary key and a repeated event
//! is ignored.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use prometheus_parking_lot_core::SchedulerError;
use prometheus_parking_lot_core::hooks::audit::{AuditEvent, AuditSink};
use sqlx::PgPool;
use tokio::sync::mpsc::{self, error::TrySendError};
use tokio::task::JoinHandle;

/// The migration that creates `pl_audit_events`.
const MIGRATION: &str = include_str!("../migrations/007_audit_events.sql");

/// Events written per statement.
const MAX_BATCH: usize = 256;

/// Insert a batch. `$1`..`$6` text arrays, `$7` event time in milliseconds.
const INSERT: &str = "INSERT INTO pl_audit_events
        (event_id, task_id, pool, tenant, action, payload, created_at)
     SELECT e.event_id, e.task_id, e.pool, e.tenant, e.action, e.payload,
            to_timestamp(e.ms / 1000.0)
       FROM UNNEST($1::text[], $2::text[], $3::text[], $4::text[], $5::text[],
                   $6::text[], $7::float8[])
            AS e(event_id, task_id, pool, tenant, action, payload, ms)
     ON CONFLICT (event_id) DO NOTHING";

/// Postgres-backed audit sink. See the [module docs](self).
#[derive(Debug)]
pub struct PostgresAuditSink {
    events: mpsc::Sender<AuditEvent>,
    lost: Arc<AtomicU64>,
    writer: JoinHandle<()>,
}

impl PostgresAuditSink {
    /// The migration statements for the audit log (one script).
    #[must_use]
    pub const fn migrations() -> &'static [&'static str] {
        &[MIGRATION]
    }

    /// Create `pl_audit_events` and its indexes if missing. Idempotent.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the migration fails.
    pub async fn migrate(pool: &PgPool) -> Result<(), SchedulerError> {
        sqlx::raw_sql(MIGRATION)
            .execute(pool)
            .await
            .map(|_| ())
            .map_err(|e| SchedulerError::backend("pg audit sink: migrate", e))
    }

    /// Start a sink that buffers up to `capacity` events (at least 1) and
    /// writes them to `pool`. The schema must exist: call
    /// [`migrate`](Self::migrate) first.
    ///
    /// Must be called inside a Tokio runtime.
    #[must_use]
    pub fn spawn(pool: PgPool, capacity: usize) -> Self {
        let (events, queue) = mpsc::channel(capacity.max(1));
        let lost = Arc::new(AtomicU64::new(0));
        let writer = tokio::spawn(write_loop(pool, queue, Arc::clone(&lost)));
        Self {
            events,
            lost,
            writer,
        }
    }

    /// Events that never reached the table: dropped because the buffer was
    /// full, or an event whose insert failed.
    #[must_use]
    pub fn lost(&self) -> u64 {
        self.lost.load(Ordering::Relaxed)
    }

    /// Stop accepting events and wait until every queued event has been
    /// written (or counted as lost). Returns [`lost`](Self::lost) as it stands.
    pub async fn shutdown(self) -> u64 {
        let Self {
            events,
            lost,
            writer,
        } = self;
        drop(events);
        if let Err(error) = writer.await {
            tracing::error!(%error, "audit writer task failed");
        }
        lost.load(Ordering::Relaxed)
    }
}

impl AuditSink for PostgresAuditSink {
    fn record(&mut self, event: AuditEvent) {
        let event_id = match self.events.try_send(event) {
            Ok(()) => return,
            Err(TrySendError::Full(event) | TrySendError::Closed(event)) => event.event_id,
        };
        self.lost.fetch_add(1, Ordering::Relaxed);
        tracing::warn!(
            event_id,
            "audit event dropped: the Postgres audit buffer is full"
        );
    }
}

async fn write_loop(pool: PgPool, mut queue: mpsc::Receiver<AuditEvent>, lost: Arc<AtomicU64>) {
    while let Some(first) = queue.recv().await {
        let mut batch = vec![first];
        while batch.len() < MAX_BATCH {
            match queue.try_recv() {
                Ok(event) => batch.push(event),
                Err(_) => break,
            }
        }
        write_batch(&pool, &batch, &lost).await;
    }
}

/// Write `batch`; count what could not be written.
///
/// A statement fails as a whole, so one event the database rejects (a NUL byte
/// in a text column, say) would take the other events of its batch with it.
/// When the database itself answered with an error, the events are retried one
/// at a time and only the rejected ones are lost. Any other failure (the
/// connection, the pool) says nothing about individual events and would repeat
/// for each of them, so there the whole batch is lost at once.
async fn write_batch(pool: &PgPool, batch: &[AuditEvent], lost: &AtomicU64) {
    let Err(error) = insert(pool, batch).await else {
        return;
    };
    if batch.len() > 1 && matches!(error, sqlx::Error::Database(_)) {
        tracing::warn!(%error, events = batch.len(), "audit batch rejected; retrying its events one by one");
        for event in batch {
            if let Err(error) = insert(pool, std::slice::from_ref(event)).await {
                lost.fetch_add(1, Ordering::Relaxed);
                tracing::error!(%error, event_id = %event.event_id, "audit event not written");
            }
        }
        return;
    }
    lost.fetch_add(batch.len() as u64, Ordering::Relaxed);
    tracing::error!(%error, events = batch.len(), "audit batch not written");
}

async fn insert(pool: &PgPool, batch: &[AuditEvent]) -> Result<(), sqlx::Error> {
    let column = |f: fn(&AuditEvent) -> String| batch.iter().map(f).collect::<Vec<_>>();
    let payloads: Vec<Option<String>> = batch.iter().map(|e| e.payload.clone()).collect();
    #[allow(
        clippy::cast_precision_loss,
        reason = "milliseconds since the epoch fit f64 exactly"
    )]
    let millis: Vec<f64> = batch.iter().map(|e| e.created_at_ms as f64).collect();
    sqlx::query(INSERT)
        .bind(column(|e| e.event_id.clone()))
        .bind(column(|e| e.task_id.clone()))
        .bind(column(|e| e.pool.clone()))
        .bind(column(|e| e.tenant.clone()))
        .bind(column(|e| e.action.clone()))
        .bind(payloads)
        .bind(millis)
        .execute(pool)
        .await
        .map(|_| ())
}
