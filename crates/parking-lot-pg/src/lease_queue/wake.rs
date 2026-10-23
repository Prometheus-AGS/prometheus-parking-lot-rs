//! Wake-up and maintenance for [`PgLeaseQueue`](super::PgLeaseQueue): the
//! `LISTEN`/`NOTIFY` listener idle dispatchers wait on, and the lease reaper.

use std::time::Duration;

use prometheus_parking_lot_core::SchedulerError;
use sqlx::postgres::PgListener;
use tokio::task::JoinHandle;

use super::PgLeaseQueue;
use super::sql::NOTIFY_CHANNEL;

/// Why [`QueueListener::wait`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Wake {
    /// Work may be available: an entry for this queue became leasable, or the
    /// listener's connection was re-established and notifications may have been
    /// missed. Call `lease`; an empty result is harmless.
    Notified,
    /// The timeout elapsed with no notification. Poll with `lease` anyway:
    /// delayed (backed-off) entries become leasable without a notification.
    TimedOut,
}

/// Waits for work on one queue.
///
/// A notification says "something became leasable"; it carries no entry. Use
/// it to cut idle polling latency, never as the only trigger: always `lease`
/// after [`Wake::TimedOut`] too.
///
/// Create the listener **before** the first `lease` of a dispatch loop.
/// Notifications that arrive while the loop is leasing are queued on the
/// listener's connection, so a wake-up between "lease returned empty" and
/// "wait" is not lost.
#[derive(Debug)]
pub struct QueueListener {
    listener: PgListener,
    queue_name: String,
}

impl QueueListener {
    pub(crate) async fn connect(
        pool: &sqlx::PgPool,
        queue_name: String,
    ) -> Result<Self, SchedulerError> {
        let mut listener = PgListener::connect_with(pool)
            .await
            .map_err(|e| SchedulerError::backend("pg lease queue: listen connect", e))?;
        listener
            .listen(NOTIFY_CHANNEL)
            .await
            .map_err(|e| SchedulerError::backend("pg lease queue: listen", e))?;
        Ok(Self {
            listener,
            queue_name,
        })
    }

    /// Wait until work may be available for this queue or `timeout` elapses.
    ///
    /// Notifications for other queues are skipped. Returns at once when one
    /// arrived while the caller was busy.
    ///
    /// **Cancellation:** not guaranteed lossless. sqlx does not promise that
    /// dropping a pending receive keeps a notification that was being read, so
    /// a cancelled `wait` may drop one. That is harmless as long as the caller
    /// polls `lease` on [`Wake::TimedOut`], which this type's contract already
    /// requires.
    ///
    /// # Errors
    ///
    /// [`SchedulerError::Backend`] when the listener connection fails and
    /// cannot be re-established.
    pub async fn wait(&mut self, timeout: Duration) -> Result<Wake, SchedulerError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            match tokio::time::timeout_at(deadline, self.listener.try_recv()).await {
                Err(_elapsed) => return Ok(Wake::TimedOut),
                // `None`: the connection was lost and re-established.
                Ok(Ok(None)) => return Ok(Wake::Notified),
                Ok(Ok(Some(note))) if note.payload() == self.queue_name => {
                    return Ok(Wake::Notified);
                }
                Ok(Ok(Some(_other_queue))) => {}
                Ok(Err(e)) => return Err(SchedulerError::backend("pg lease queue: wait", e)),
            }
        }
    }
}

/// What one reaper pass did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Reaped {
    /// Expired leases returned to the ready set.
    pub requeued: u64,
    /// Expired leases dead-lettered because the entry used up its attempts.
    pub dead_lettered: u64,
}

/// Stops the background reaper started by
/// [`PgLeaseQueue::spawn_reaper`] when dropped.
#[derive(Debug)]
#[must_use = "the reaper stops when its handle is dropped"]
pub struct ReaperHandle {
    task: JoinHandle<()>,
}

impl Drop for ReaperHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl<P: Send + Sync + 'static> PgLeaseQueue<P> {
    /// Start a task that calls [`reap_expired`](Self::reap_expired) every
    /// `interval` until the returned handle is dropped.
    ///
    /// Must be called inside a Tokio runtime. A failed pass is logged and
    /// retried on the next tick.
    pub fn spawn_reaper(&self, interval: Duration) -> ReaperHandle {
        let queue = self.clone();
        let task = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                ticker.tick().await;
                if let Err(error) = queue.reap_expired().await {
                    tracing::warn!(%error, queue = queue.queue_name(), "lease reaper pass failed");
                }
            }
        });
        ReaperHandle { task }
    }
}
