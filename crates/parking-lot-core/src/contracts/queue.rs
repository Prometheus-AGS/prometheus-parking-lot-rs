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
    /// Remove expired tasks and return count.
    ///
    /// A task is expired once `now_ms` is past its `deadline_ms`
    /// (`now_ms > deadline_ms`); at its deadline it is still live. This is
    /// the rule the dispatcher applies (`Ticket::is_expired`).
    ///
    /// # Errors
    ///
    /// Returns an error when the backend fails to remove expired tasks.
    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError>;
    /// Maximum depth allowed for this queue.
    fn max_depth(&self) -> usize;
    /// Current depth.
    fn len(&self) -> usize;
    /// True when the queue holds no tasks. Defaults to `len() == 0`.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
