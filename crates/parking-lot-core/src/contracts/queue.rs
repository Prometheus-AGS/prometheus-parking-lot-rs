//! Queue backend contract.

use crate::contracts::error::SchedulerError;
use crate::contracts::task::ScheduledTask;

/// Abstraction for queue backends.
pub trait TaskQueue<P> {
    /// Enqueue a task if space permits.
    ///
    /// # Errors
    ///
    /// Returns an error when the queue is full or the backend fails to store
    /// the task.
    fn enqueue(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError>;
    /// Dequeue the next ready task, honoring priority and deadlines.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails to read or remove the task.
    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError>;
    /// Remove expired tasks and return every removed task, with its original
    /// metadata and payload, exactly once.
    ///
    /// A task is expired once `now_ms` is past its `deadline_ms`
    /// (`now_ms > deadline_ms`); at its deadline it is still live. This is
    /// the rule the dispatcher applies (`Ticket::is_expired`).
    /// Tasks without a deadline and tasks that are live must remain queued
    /// with their ordering metadata unchanged. A repeated prune cannot return
    /// an already removed task. The returned order is unspecified.
    ///
    /// The caller uses the returned metadata to report each task's terminal
    /// expiry and settle its accepted-work accounting outside queue/dispatcher
    /// locks. Returning only a count or silently dropping a removed task would
    /// lose that obligation. No payload cloning is required.
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails to remove expired tasks. An
    /// error must leave every task queued: a backend must not remove tasks
    /// whose metadata it cannot return. Adapters must finish fallible backend
    /// pruning before mutating their own infallible buffer.
    fn prune_expired(&mut self, now_ms: u128) -> Result<Vec<ScheduledTask<P>>, SchedulerError>;
    /// Maximum depth allowed for this queue.
    fn max_depth(&self) -> usize;
    /// Current depth.
    fn len(&self) -> usize;
    /// True when the queue holds no tasks. Defaults to `len() == 0`.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
