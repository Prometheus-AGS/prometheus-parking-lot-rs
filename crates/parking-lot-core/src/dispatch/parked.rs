//! Parked-task storage for the [`Dispatcher`](super::Dispatcher).
//!
//! The dispatcher needs to look at the head of the parked queue before it
//! decides to take it ("peek, then pop only when the head fits"), so that a
//! head that does not fit never leaves the queue. [`ParkedQueue`] is that
//! interface. Two implementations:
//!
//! - [`PriorityQueue`]: an in-memory heap, used by the worker pools and the
//!   loom models.
//! - [`TaskQueueAdapter`]: puts a peek in front of any
//!   [`TaskQueue`] backend, whose contract offers only `dequeue`, without ever
//!   putting a task back into the backend.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::contracts::{Priority, ScheduledTask, SchedulerError, TaskQueue};

use super::WorkerTask;

/// The scheduling facts of a parked item: everything the dispatcher reads to
/// order, admit or expire it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    /// Queue priority (higher runs first).
    pub priority: Priority,
    /// Submission sequence stamped by the dispatcher: FIFO within a priority.
    pub sequence: u64,
    /// Units the item needs from the semaphore.
    pub units: u32,
    /// Absolute deadline (wall-clock ms) after which the item must not start.
    pub deadline_ms: Option<u128>,
}

impl Ticket {
    /// Ordering key: higher priority, then lower sequence, is greater (the
    /// max-heap pops the greatest).
    const fn key(&self) -> (Priority, Reverse<u64>) {
        (self.priority, Reverse(self.sequence))
    }

    /// Whether this item must be served before `other`.
    #[must_use]
    pub fn runs_before(&self, other: &Self) -> bool {
        self.key() > other.key()
    }

    /// Whether the deadline has passed at `now_ms`.
    #[must_use]
    pub fn is_expired(&self, now_ms: u128) -> bool {
        self.deadline_ms.is_some_and(|deadline| now_ms > deadline)
    }
}

/// An item the dispatcher can park: it reports its [`Ticket`] and accepts the
/// sequence number the dispatcher stamps on it.
pub trait Parkable {
    /// The item's scheduling facts.
    fn ticket(&self) -> Ticket;
    /// Record the submission sequence the dispatcher assigned.
    fn set_sequence(&mut self, sequence: u64);
}

impl<P> Parkable for ScheduledTask<P> {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: self.meta.priority,
            sequence: self.meta.sequence,
            units: self.meta.cost.total_units(),
            deadline_ms: self.meta.deadline_ms,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.meta.sequence = sequence;
    }
}

impl<P> Parkable for WorkerTask<P> {
    fn ticket(&self) -> Ticket {
        Ticket {
            priority: self.meta.priority,
            sequence: self.meta.sequence,
            units: self.meta.cost.total_units(),
            deadline_ms: self.meta.deadline_ms,
        }
    }

    fn set_sequence(&mut self, sequence: u64) {
        self.meta.sequence = sequence;
    }
}

/// Storage for parked items, ordered by [`Ticket`] (priority, then FIFO).
///
/// Every method is called with the dispatcher's lock held.
pub trait ParkedQueue<T> {
    /// Number of parked items.
    fn len(&self) -> usize;

    /// True when nothing is parked.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Park `item`.
    ///
    /// # Errors
    ///
    /// The backend's error when it cannot store the item; the item is then not
    /// parked and the submission fails.
    fn push(&mut self, item: T) -> Result<(), SchedulerError>;

    /// The ticket of the item that must be served next, without removing it.
    ///
    /// # Errors
    ///
    /// The backend's error when it cannot be read; nothing is removed.
    fn head(&mut self) -> Result<Option<Ticket>, SchedulerError>;

    /// Remove and return the item whose ticket [`head`](Self::head) just
    /// returned. Infallible: `head` already brought it into reach.
    fn pop_head(&mut self) -> Option<T>;

    /// Remove every parked item whose deadline passed at `now_ms`; return how
    /// many were removed.
    ///
    /// # Errors
    ///
    /// The backend's error when pruning fails.
    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError>;
}

/// Heap entry ordered by its ticket alone (sequences are unique per
/// dispatcher, so the order is total and `Eq` agrees with `Ord`).
#[derive(Debug)]
struct Entry<T> {
    ticket: Ticket,
    item: T,
}

impl<T> PartialEq for Entry<T> {
    fn eq(&self, other: &Self) -> bool {
        self.ticket.key() == other.ticket.key()
    }
}

impl<T> Eq for Entry<T> {}

impl<T> PartialOrd for Entry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Entry<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ticket.key().cmp(&other.ticket.key())
    }
}

/// In-memory parked queue: a binary heap keyed by [`Ticket`].
#[derive(Debug)]
pub struct PriorityQueue<T> {
    heap: BinaryHeap<Entry<T>>,
}

impl<T> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self {
            heap: BinaryHeap::new(),
        }
    }
}

impl<T: Parkable> PriorityQueue<T> {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The ticket that must be served last, if any (O(n); used only when the
    /// adapter refills its front).
    fn last_ticket(&self) -> Option<Ticket> {
        self.heap
            .iter()
            .map(|entry| entry.ticket)
            .min_by_key(Ticket::key)
    }

    /// Remove and return every parked item, in no particular order.
    pub fn take_all(&mut self) -> Vec<T> {
        self.heap.drain().map(|entry| entry.item).collect()
    }
}

impl<T: Parkable> ParkedQueue<T> for PriorityQueue<T> {
    fn len(&self) -> usize {
        self.heap.len()
    }

    fn push(&mut self, item: T) -> Result<(), SchedulerError> {
        self.heap.push(Entry {
            ticket: item.ticket(),
            item,
        });
        Ok(())
    }

    fn head(&mut self) -> Result<Option<Ticket>, SchedulerError> {
        Ok(self.heap.peek().map(|entry| entry.ticket))
    }

    fn pop_head(&mut self) -> Option<T> {
        self.heap.pop().map(|entry| entry.item)
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        let before = self.heap.len();
        self.heap.retain(|entry| !entry.ticket.is_expired(now_ms));
        Ok(before - self.heap.len())
    }
}

/// A [`ParkedQueue`] over a [`TaskQueue`] backend.
///
/// The backend contract has `dequeue` but no peek, and putting a task that does
/// not fit back with `enqueue` can fail (or reorder it), which is how the 0.2
/// pool lost tasks (audit C4). The adapter instead keeps a small in-memory
/// *front* and maintains one invariant:
///
/// > every task in the front is served before every task in the backend.
///
/// - The head is the front's best task. When the front is empty, the
///   backend's best task is dequeued into it, which preserves the invariant.
/// - A new task that must be served before the front's *last* task joins the
///   front (it then precedes everything in the backend too); any other new
///   task goes to the backend (it follows everything in the front).
///
/// A task leaves the adapter only through `pop_head`, after the dispatcher
/// admitted it, so no task is ever pushed back into the backend.
///
/// Tasks in the front are held in memory, exactly like a task the 0.2 pool
/// had dequeued and not yet started; durable leasing of the head is cr-49.
pub struct TaskQueueAdapter<P, Q> {
    front: PriorityQueue<ScheduledTask<P>>,
    /// The front's last ticket; `None` exactly when the front is empty.
    front_last: Option<Ticket>,
    backend: Q,
}

impl<P, Q> TaskQueueAdapter<P, Q> {
    /// Wrap `backend`.
    pub fn new(backend: Q) -> Self {
        Self {
            front: PriorityQueue::default(),
            front_last: None,
            backend,
        }
    }
}

impl<P, Q: TaskQueue<P>> ParkedQueue<ScheduledTask<P>> for TaskQueueAdapter<P, Q> {
    fn len(&self) -> usize {
        self.front.len() + self.backend.len()
    }

    fn push(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError> {
        let ticket = task.ticket();
        match self.front_last {
            Some(last) if ticket.runs_before(&last) => self.front.push(task),
            _ => self.backend.enqueue(task),
        }
    }

    fn head(&mut self) -> Result<Option<Ticket>, SchedulerError> {
        if self.front.is_empty() {
            let Some(task) = self.backend.dequeue()? else {
                return Ok(None);
            };
            self.front_last = Some(task.ticket());
            self.front.push(task)?;
        }
        self.front.head()
    }

    fn pop_head(&mut self) -> Option<ScheduledTask<P>> {
        let task = self.front.pop_head();
        if self.front.is_empty() {
            self.front_last = None;
        }
        task
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        let from_front = self.front.prune_expired(now_ms)?;
        if from_front > 0 {
            self.front_last = self.front.last_ticket();
        }
        Ok(from_front + self.backend.prune_expired(now_ms)?)
    }
}

#[cfg(all(test, not(parking_lot_loom)))]
mod tests {
    use super::*;
    use crate::contracts::{InMemoryQueue, ResourceCost, ResourceKind, TaskMetadata};

    fn task(sequence: u64, priority: Priority) -> ScheduledTask<u64> {
        ScheduledTask {
            meta: TaskMetadata {
                id: sequence,
                mailbox: None,
                priority,
                cost: ResourceCost::new(ResourceKind::Cpu, 1),
                deadline_ms: None,
                created_at_ms: 0,
                idempotency_key: None,
                sequence,
            },
            payload: sequence,
        }
    }

    fn drain<Q: ParkedQueue<ScheduledTask<u64>>>(queue: &mut Q) -> Vec<u64> {
        let mut order = Vec::new();
        while queue.head().expect("head").is_some() {
            order.push(queue.pop_head().expect("head was present").payload);
        }
        order
    }

    #[test]
    fn priority_queue_serves_priority_then_fifo() {
        let mut queue = PriorityQueue::new();
        for (sequence, priority) in [
            (1, Priority::Low),
            (2, Priority::High),
            (3, Priority::Low),
            (4, Priority::High),
        ] {
            queue.push(task(sequence, priority)).expect("push");
        }
        assert_eq!(drain(&mut queue), vec![2, 4, 1, 3]);
    }

    /// A task that arrives after the head was pulled into the front still
    /// overtakes it when it has a higher priority, and nothing is put back.
    #[test]
    fn adapter_keeps_priority_order_across_the_front() {
        let mut queue = TaskQueueAdapter::new(InMemoryQueue::new(16));
        queue.push(task(1, Priority::Low)).expect("push");
        queue.push(task(2, Priority::Normal)).expect("push");
        // Pull the head (2) into the front.
        assert_eq!(queue.head().expect("head").map(|t| t.sequence), Some(2));
        queue.push(task(3, Priority::Critical)).expect("push");
        queue.push(task(4, Priority::Normal)).expect("push");
        queue.push(task(5, Priority::Low)).expect("push");
        assert_eq!(queue.len(), 5);
        assert_eq!(drain(&mut queue), vec![3, 2, 4, 1, 5]);
    }

    #[test]
    fn adapter_prunes_front_and_backend() {
        let mut queue = TaskQueueAdapter::new(InMemoryQueue::new(16));
        let mut expiring = task(1, Priority::High);
        expiring.meta.deadline_ms = Some(10);
        queue.push(expiring).expect("push");
        queue.push(task(2, Priority::Low)).expect("push");
        let mut later = task(3, Priority::Low);
        later.meta.deadline_ms = Some(10);
        queue.push(later).expect("push");
        assert!(queue.head().expect("head").is_some());
        assert_eq!(queue.prune_expired(11).expect("prune"), 2);
        assert_eq!(drain(&mut queue), vec![2]);
    }
}
