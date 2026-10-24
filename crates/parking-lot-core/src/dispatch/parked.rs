//! Parked-task storage for the [`Dispatcher`](super::Dispatcher).
//!
//! The dispatcher needs to look at the head of the parked queue before it
//! decides to take it ("peek, then pop only when the head fits"), so that a
//! head that does not fit never leaves the queue. With skip-ahead (cr-20) it
//! also looks *behind* a head that does not fit for the first item that does
//! ([`ParkedQueue::fitting`]) and takes exactly that item
//! ([`ParkedQueue::take`]). [`ParkedQueue`] is that interface. Two
//! implementations:
//!
//! - [`PriorityQueue`]: an in-memory ordered map, used by the worker pools and
//!   the loom models.
//! - [`TaskQueueAdapter`]: puts a peek in front of any
//!   [`TaskQueue`] backend, whose contract offers only `dequeue`, without ever
//!   putting a task back into the backend.

use std::cmp::Reverse;
use std::collections::BTreeMap;

use crate::contracts::{Priority, ScheduledTask, SchedulerError, TaskQueue};
use crate::semaphore::UnitVector;

use super::WorkerTask;

/// The scheduling facts of a parked item: everything the dispatcher reads to
/// order, admit or expire it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ticket {
    /// Queue priority (higher runs first).
    pub priority: Priority,
    /// Submission sequence stamped by the dispatcher: FIFO within a priority.
    pub sequence: u64,
    /// Units the item needs from the semaphore, in every dimension (cr-23).
    /// [`UnitVector::scalar`] for an item with a single unit count.
    pub demand: UnitVector,
    /// Absolute deadline (wall-clock ms) after which the item must not start.
    pub deadline_ms: Option<u128>,
}

impl Ticket {
    /// Ordering key: higher priority, then lower sequence, is greater.
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

    /// Units the item needs in the total dimension.
    #[must_use]
    pub const fn units(&self) -> u32 {
        self.demand.total()
    }

    /// Whether the item can start now in place of a blocked head: not
    /// expired, and its demand fits `free` in every dimension.
    #[must_use]
    pub fn can_skip_ahead(&self, free: &UnitVector, now_ms: u128) -> bool {
        self.demand.fits_in(free) && !self.is_expired(now_ms)
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
            demand: UnitVector::from_cost(&self.meta.cost),
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
            demand: UnitVector::from_cost(&self.meta.cost),
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

    /// Skip-ahead (cr-20): the ticket of the first item, in dispatch order,
    /// that is not expired at `now_ms` and whose demand fits `free` in every
    /// dimension ([`Ticket::can_skip_ahead`]), without removing it. The
    /// dispatcher calls it only when the head does not fit, so the head itself
    /// is not normally a candidate.
    ///
    /// The default finds nothing: a queue that does not implement it keeps
    /// strict head-of-line order.
    ///
    /// # Errors
    ///
    /// The backend's error when it cannot be read; nothing is removed.
    fn fitting(
        &mut self,
        free: &UnitVector,
        now_ms: u128,
    ) -> Result<Option<Ticket>, SchedulerError> {
        let _ = (free, now_ms);
        Ok(None)
    }

    /// Remove and return an item whose ticket equals `ticket`, which
    /// [`fitting`](Self::fitting) just returned. Infallible for the same
    /// reason as [`pop_head`](Self::pop_head).
    ///
    /// The default removes nothing (it pairs with the default `fitting`).
    fn take(&mut self, ticket: &Ticket) -> Option<T> {
        let _ = ticket;
        None
    }

    /// Whether this queue implements [`fitting`](Self::fitting) and
    /// [`take`](Self::take), so a blocked head may be skipped. The
    /// dispatcher reads it to decide whether a submission that arrives with
    /// the queue at its depth limit may start as a skip; a `None` from
    /// `fitting` cannot tell "nothing fits" from "not implemented".
    ///
    /// The default is `false`, which pairs with the defaults above. A queue
    /// that implements them but leaves this `false` still skips ahead in the
    /// drain; only the at-depth start is then refused (`QueueFull`).
    fn supports_skip_ahead(&self) -> bool {
        false
    }

    /// Remove every parked item whose deadline passed at `now_ms` and return
    /// each removed item exactly once with its original metadata/payload.
    /// Equality at the deadline is live; items without deadlines stay parked.
    /// Survivor ordering is unchanged. Removed-item order is unspecified and
    /// no payload cloning is required. The caller owns terminal reporting.
    ///
    /// # Errors
    ///
    /// The backend's error when pruning fails; nothing is removed on error.
    fn prune_expired(&mut self, now_ms: u128) -> Result<Vec<T>, SchedulerError>;
}

/// Map key: dispatch order (higher priority, then lower sequence) first, then
/// an arrival number, so items with equal tickets never collide.
type Key = (Reverse<Priority>, u64, u64);

/// A parked item and its ticket.
#[derive(Debug)]
struct Entry<T> {
    ticket: Ticket,
    item: T,
}

/// In-memory parked queue: an ordered map keyed by dispatch order.
///
/// The head and removals are `O(log n)`. The skip-ahead lookup walks the
/// queue in dispatch order and stops at the first item that fits; it returns
/// at once when the smallest parked total exceeds the free total, which it
/// reads from a per-cost index (a demand fits only if its total does).
#[derive(Debug)]
pub struct PriorityQueue<T> {
    entries: BTreeMap<Key, Entry<T>>,
    /// Arrival counter for [`Key`]'s tie-break.
    arrivals: u64,
    /// Number of parked items per total cost in units.
    costs: BTreeMap<u32, usize>,
}

impl<T> Default for PriorityQueue<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            arrivals: 0,
            costs: BTreeMap::new(),
        }
    }
}

impl<T> PriorityQueue<T> {
    /// Count one more (`added`) or one fewer parked item costing `units`.
    fn count_cost(&mut self, units: u32, added: bool) {
        if added {
            *self.costs.entry(units).or_insert(0) += 1;
        } else if let Some(count) = self.costs.get_mut(&units) {
            *count -= 1;
            if *count == 0 {
                self.costs.remove(&units);
            }
        }
    }

    /// Remove the entry under `key`, keeping the cost index in step.
    fn remove(&mut self, key: &Key) -> Option<T> {
        let entry = self.entries.remove(key)?;
        self.count_cost(entry.ticket.units(), false);
        Some(entry.item)
    }

    /// The ticket that must be served last, if any.
    fn last_ticket(&self) -> Option<Ticket> {
        self.entries.values().next_back().map(|entry| entry.ticket)
    }
}

impl<T: Parkable> PriorityQueue<T> {
    /// An empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Remove and return every parked item, in dispatch order.
    pub fn take_all(&mut self) -> Vec<T> {
        self.costs.clear();
        std::mem::take(&mut self.entries)
            .into_values()
            .map(|entry| entry.item)
            .collect()
    }
}

impl<T: Parkable> ParkedQueue<T> for PriorityQueue<T> {
    fn len(&self) -> usize {
        self.entries.len()
    }

    fn push(&mut self, item: T) -> Result<(), SchedulerError> {
        let ticket = item.ticket();
        self.arrivals += 1;
        let key = (Reverse(ticket.priority), ticket.sequence, self.arrivals);
        self.count_cost(ticket.units(), true);
        self.entries.insert(key, Entry { ticket, item });
        Ok(())
    }

    fn head(&mut self) -> Result<Option<Ticket>, SchedulerError> {
        Ok(self.entries.values().next().map(|entry| entry.ticket))
    }

    fn pop_head(&mut self) -> Option<T> {
        let (_, entry) = self.entries.pop_first()?;
        self.count_cost(entry.ticket.units(), false);
        Some(entry.item)
    }

    fn fitting(
        &mut self,
        free: &UnitVector,
        now_ms: u128,
    ) -> Result<Option<Ticket>, SchedulerError> {
        let smallest = self.costs.first_key_value().map(|(&units, _)| units);
        if smallest.is_none_or(|units| units > free.total()) {
            return Ok(None);
        }
        Ok(self
            .entries
            .values()
            .find(|entry| entry.ticket.can_skip_ahead(free, now_ms))
            .map(|entry| entry.ticket))
    }

    fn supports_skip_ahead(&self) -> bool {
        true
    }

    fn take(&mut self, ticket: &Ticket) -> Option<T> {
        let order = Reverse(ticket.priority);
        let key = self
            .entries
            .range((order, ticket.sequence, 0)..=(order, ticket.sequence, u64::MAX))
            .find(|(_, entry)| entry.ticket == *ticket)
            .map(|(key, _)| *key)?;
        self.remove(&key)
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<Vec<T>, SchedulerError> {
        let expired: Vec<Key> = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.ticket.is_expired(now_ms))
            .map(|(key, _)| *key)
            .collect();
        let mut removed = Vec::with_capacity(expired.len());
        for key in expired {
            if let Some(item) = self.remove(&key) {
                removed.push(item);
            }
        }
        Ok(removed)
    }
}

/// How many tasks a [`TaskQueueAdapter`] may hold in its in-memory front while
/// it looks behind a blocked head for one that fits.
///
/// The lookup moves tasks from the backend into the front (never back), so
/// this bounds how many backend tasks a blocked head can pull into memory and
/// how long one lookup takes. Tasks further back are not considered until the
/// front shrinks.
pub const ADAPTER_LOOKAHEAD: usize = 64;

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
/// - The skip-ahead lookup searches the front first. When nothing there fits
///   it dequeues the backend's best task into the front (which preserves the
///   invariant: that task follows everything already in the front) and checks
///   it, until one fits or the front holds [`ADAPTER_LOOKAHEAD`] tasks.
///
/// A task leaves the adapter through `pop_head`, `take` or `prune_expired`,
/// after the dispatcher admitted, expired or rejected it. Pruning returns the
/// full removed tasks, so no task is ever pushed back into the backend or
/// discarded without its terminal-reporting metadata.
///
/// Tasks in the front are held in memory, exactly like a task the 0.2 pool
/// had dequeued and not yet started; durable leasing of the head is cr-49.
pub struct TaskQueueAdapter<P, Q> {
    front: PriorityQueue<ScheduledTask<P>>,
    backend: Q,
}

impl<P, Q> TaskQueueAdapter<P, Q> {
    /// Wrap `backend`.
    pub fn new(backend: Q) -> Self {
        Self {
            front: PriorityQueue::default(),
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
        match self.front.last_ticket() {
            Some(last) if ticket.runs_before(&last) => self.front.push(task),
            _ => self.backend.enqueue(task),
        }
    }

    fn head(&mut self) -> Result<Option<Ticket>, SchedulerError> {
        if self.front.is_empty() {
            let Some(task) = self.backend.dequeue()? else {
                return Ok(None);
            };
            self.front.push(task)?;
        }
        self.front.head()
    }

    fn pop_head(&mut self) -> Option<ScheduledTask<P>> {
        self.front.pop_head()
    }

    fn fitting(
        &mut self,
        free: &UnitVector,
        now_ms: u128,
    ) -> Result<Option<Ticket>, SchedulerError> {
        if let Some(ticket) = self.front.fitting(free, now_ms)? {
            return Ok(Some(ticket));
        }
        while self.front.len() < ADAPTER_LOOKAHEAD {
            let Some(task) = self.backend.dequeue()? else {
                return Ok(None);
            };
            let ticket = task.ticket();
            self.front.push(task)?;
            if ticket.can_skip_ahead(free, now_ms) {
                return Ok(Some(ticket));
            }
        }
        Ok(None)
    }

    fn take(&mut self, ticket: &Ticket) -> Option<ScheduledTask<P>> {
        self.front.take(ticket)
    }

    fn supports_skip_ahead(&self) -> bool {
        true
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<Vec<ScheduledTask<P>>, SchedulerError> {
        // The backend can fail; the in-memory front cannot. Finish fallible
        // pruning before mutating the front so Err never loses buffered work.
        let mut removed = self.backend.prune_expired(now_ms)?;
        removed.extend(self.front.prune_expired(now_ms)?);
        Ok(removed)
    }
}

#[cfg(all(test, not(parking_lot_loom)))]
mod tests {
    use super::*;
    use crate::contracts::{InMemoryQueue, ResourceCost, ResourceKind, TaskMetadata};

    fn task(sequence: u64, priority: Priority) -> ScheduledTask<u64> {
        sized(sequence, priority, 1)
    }

    fn sized(sequence: u64, priority: Priority, units: u32) -> ScheduledTask<u64> {
        ScheduledTask {
            meta: TaskMetadata {
                id: sequence,
                mailbox: None,
                priority,
                cost: ResourceCost::new(ResourceKind::Cpu, units),
                deadline_ms: None,
                created_at_ms: 0,
                idempotency_key: None,
                sequence,
            },
            payload: sequence,
        }
    }

    /// Free capacity of `units` in the total dimension, no kind limits.
    const fn free(units: u32) -> UnitVector {
        UnitVector::free(units, [u32::MAX; crate::semaphore::KIND_COUNT])
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

    /// Items with equal tickets (unsequenced, same priority) are all kept.
    #[test]
    fn priority_queue_keeps_items_with_equal_tickets() {
        let mut queue = PriorityQueue::new();
        for _ in 0..3 {
            queue.push(task(0, Priority::Normal)).expect("push");
        }
        assert_eq!(queue.len(), 3);
        assert_eq!(drain(&mut queue).len(), 3);
    }

    /// The lookup returns the best item that fits and is not expired, and
    /// `take` removes exactly that item.
    #[test]
    fn priority_queue_fitting_finds_the_best_item_that_fits() {
        let mut queue = PriorityQueue::new();
        queue.push(sized(1, Priority::High, 4)).expect("push");
        let mut late = sized(2, Priority::High, 1);
        late.meta.deadline_ms = Some(5);
        queue.push(late).expect("push");
        queue.push(sized(3, Priority::Low, 1)).expect("push");
        queue.push(sized(4, Priority::Normal, 2)).expect("push");
        assert_eq!(queue.fitting(&free(0), 10).expect("fitting"), None);
        let found = queue
            .fitting(&free(2), 10)
            .expect("fitting")
            .expect("4 fits");
        assert_eq!(found.sequence, 4);
        assert_eq!(queue.take(&found).map(|t| t.payload), Some(4));
        let found = queue
            .fitting(&free(2), 10)
            .expect("fitting")
            .expect("3 fits");
        assert_eq!(found.sequence, 3);
        assert_eq!(queue.take(&found).map(|t| t.payload), Some(3));
        assert_eq!(drain(&mut queue), vec![1, 2]);
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

    /// The skip-ahead lookup pulls backend tasks into the front in order, and
    /// the order of everything left is unchanged.
    #[test]
    fn adapter_fitting_pulls_from_the_backend_in_order() {
        let mut queue = TaskQueueAdapter::new(InMemoryQueue::new(16));
        queue.push(sized(1, Priority::High, 4)).expect("push");
        queue.push(sized(2, Priority::Normal, 3)).expect("push");
        queue.push(sized(3, Priority::Normal, 1)).expect("push");
        queue.push(sized(4, Priority::Low, 1)).expect("push");
        assert_eq!(queue.head().expect("head").map(|t| t.sequence), Some(1));
        let found = queue
            .fitting(&free(1), 0)
            .expect("fitting")
            .expect("3 fits");
        assert_eq!(found.sequence, 3);
        assert_eq!(queue.take(&found).map(|t| t.payload), Some(3));
        // A task that must precede the front's last (2) joins the front; one
        // that follows it goes to the backend.
        queue.push(sized(5, Priority::High, 1)).expect("push");
        queue.push(sized(6, Priority::Low, 1)).expect("push");
        assert_eq!(queue.len(), 5);
        assert_eq!(drain(&mut queue), vec![1, 5, 2, 4, 6]);
    }

    #[test]
    fn adapter_lookahead_is_bounded() {
        let mut queue = TaskQueueAdapter::new(InMemoryQueue::new(1024));
        let total = u64::try_from(ADAPTER_LOOKAHEAD).expect("small") + 8;
        for sequence in 1..=total {
            queue
                .push(sized(sequence, Priority::Normal, 2))
                .expect("push");
        }
        queue
            .push(sized(total + 1, Priority::Normal, 1))
            .expect("push");
        assert!(queue.head().expect("head").is_some());
        // Only ADAPTER_LOOKAHEAD tasks are examined; the one-unit task lies
        // beyond the window.
        assert_eq!(queue.fitting(&free(1), 0).expect("fitting"), None);
        assert_eq!(queue.front.len(), ADAPTER_LOOKAHEAD);
        assert_eq!(queue.len(), usize::try_from(total).expect("small") + 1);
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
        let mut removed: Vec<_> = queue
            .prune_expired(11)
            .expect("prune")
            .into_iter()
            .map(|task| task.meta.id)
            .collect();
        removed.sort_unstable();
        assert_eq!(removed, vec![1, 3]);
        assert_eq!(drain(&mut queue), vec![2]);
    }
}
