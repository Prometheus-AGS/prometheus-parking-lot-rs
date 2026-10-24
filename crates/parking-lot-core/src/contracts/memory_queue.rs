//! In-memory queue with priority and deadline awareness.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::contracts::SchedulerError;
use crate::contracts::types::Priority;
use crate::contracts::{ScheduledTask, TaskQueue};

/// Heap entry ordering a task by priority (highest first), then by
/// submission sequence, creation time and id (lowest first).
///
/// `Eq` compares exactly the keys `Ord` compares, so the two agree (two
/// entries are equal only when every ordering key is equal).
struct PriorityTask<P> {
    task: ScheduledTask<P>,
}

impl<P> PriorityTask<P> {
    /// The ordering key. Earlier sequence, creation time and id must sort
    /// *greater* for the max-heap, hence the `Reverse`s.
    const fn key(&self) -> (Priority, Reverse<u64>, Reverse<u128>, Reverse<u64>) {
        let meta = &self.task.meta;
        (
            meta.priority,
            Reverse(meta.sequence),
            Reverse(meta.created_at_ms),
            Reverse(meta.id),
        )
    }
}

impl<P> PartialEq for PriorityTask<P> {
    fn eq(&self, other: &Self) -> bool {
        self.key() == other.key()
    }
}

impl<P> Eq for PriorityTask<P> {}

impl<P> PartialOrd for PriorityTask<P> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<P> Ord for PriorityTask<P> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.key().cmp(&other.key())
    }
}

/// In-memory queue storing scheduled tasks using a priority heap.
///
/// Dequeues by priority (highest first), then FIFO by
/// [`TaskMetadata::sequence`](crate::contracts::TaskMetadata), with creation
/// time and id as tie-breakers for unsequenced tasks. Enqueue and dequeue are
/// O(log n).
pub struct InMemoryQueue<P> {
    max_depth: usize,
    /// Binary heap for O(log n) priority-based operations.
    tasks: BinaryHeap<PriorityTask<P>>,
}

impl<P> InMemoryQueue<P> {
    /// Create a new in-memory queue with a maximum depth.
    #[must_use]
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            tasks: BinaryHeap::with_capacity(max_depth.min(1024)),
        }
    }
}

impl<P> TaskQueue<P> for InMemoryQueue<P> {
    fn enqueue(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError> {
        if self.len() >= self.max_depth() {
            return Err(SchedulerError::queue_full(
                "in-memory",
                self.len(),
                self.max_depth,
            ));
        }
        // O(log n) insertion
        self.tasks.push(PriorityTask { task });
        Ok(())
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError> {
        // O(log n) removal
        Ok(self.tasks.pop().map(|pt| pt.task))
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<Vec<ScheduledTask<P>>, SchedulerError> {
        // Move removed tasks to the caller, preserving all metadata without
        // requiring P: Clone. Equality at the deadline is still live.
        let mut expired = Vec::new();
        let tasks = std::mem::take(&mut self.tasks);
        self.tasks = tasks
            .into_iter()
            .filter_map(|task| {
                if task
                    .task
                    .meta
                    .deadline_ms
                    .is_some_and(|deadline| now_ms > deadline)
                {
                    expired.push(task.task);
                    None
                } else {
                    Some(task)
                }
            })
            .collect();
        Ok(expired)
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    fn len(&self) -> usize {
        self.tasks.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::types::{ResourceCost, ResourceKind};

    fn make_task(id: u64, priority: Priority, created_at_ms: u128) -> ScheduledTask<String> {
        ScheduledTask {
            meta: crate::contracts::TaskMetadata {
                id,
                mailbox: None,
                priority,
                cost: ResourceCost::new(ResourceKind::Cpu, 1),
                deadline_ms: None,
                created_at_ms,
                idempotency_key: None,
                sequence: 0,
            },
            payload: format!("task-{id}"),
        }
    }

    #[test]
    fn test_priority_ordering() {
        let mut q = InMemoryQueue::new(100);

        // Enqueue in mixed order
        q.enqueue(make_task(1, Priority::Low, 100)).unwrap();
        q.enqueue(make_task(2, Priority::Critical, 200)).unwrap();
        q.enqueue(make_task(3, Priority::Normal, 300)).unwrap();
        q.enqueue(make_task(4, Priority::High, 400)).unwrap();

        // Should dequeue in priority order
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 2); // Critical
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 4); // High
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 3); // Normal
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 1); // Low
    }

    #[test]
    fn test_fifo_within_priority() {
        let mut q = InMemoryQueue::new(100);

        // Enqueue same priority, different times
        q.enqueue(make_task(1, Priority::Normal, 300)).unwrap();
        q.enqueue(make_task(2, Priority::Normal, 100)).unwrap();
        q.enqueue(make_task(3, Priority::Normal, 200)).unwrap();

        // Should dequeue FIFO within same priority
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 2); // created_at=100
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 3); // created_at=200
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 1); // created_at=300
    }

    #[test]
    fn sequence_orders_fifo_within_priority_regardless_of_timestamp() {
        let mut q = InMemoryQueue::new(2_000);
        // Every task shares one timestamp and the ids run backwards, so only
        // the sequence can produce submission order.
        for seq in 1..=1_000_u64 {
            let mut task = make_task(10_000 - seq, Priority::Normal, 42);
            task.meta.sequence = seq;
            q.enqueue(task).unwrap();
        }
        let mut high = make_task(1, Priority::High, 42);
        high.meta.sequence = 1_001;
        q.enqueue(high).unwrap();

        assert_eq!(q.dequeue().unwrap().unwrap().meta.sequence, 1_001);
        for expected in 1..=1_000_u64 {
            assert_eq!(q.dequeue().unwrap().unwrap().meta.sequence, expected);
        }
    }

    #[test]
    fn requeued_task_keeps_its_place() {
        let mut q = InMemoryQueue::new(10);
        for seq in 1..=3_u64 {
            let mut task = make_task(seq, Priority::Normal, 0);
            task.meta.sequence = seq;
            q.enqueue(task).unwrap();
        }
        let head = q.dequeue().unwrap().unwrap();
        assert_eq!(head.meta.sequence, 1);
        q.enqueue(head).unwrap();
        assert_eq!(q.dequeue().unwrap().unwrap().meta.sequence, 1);
    }

    #[test]
    fn test_queue_full() {
        let mut q = InMemoryQueue::new(2);
        q.enqueue(make_task(1, Priority::Normal, 100)).unwrap();
        q.enqueue(make_task(2, Priority::Normal, 200)).unwrap();

        let result = q.enqueue(make_task(3, Priority::Normal, 300));
        assert!(result.is_err());
    }

    #[test]
    fn test_prune_expired() {
        let mut q = InMemoryQueue::new(100);

        // Task 1: no deadline (should remain)
        q.enqueue(make_task(1, Priority::Normal, 100)).unwrap();

        // Task 2: deadline in past (should be pruned)
        let mut task2 = make_task(2, Priority::High, 200);
        task2.meta.deadline_ms = Some(500);
        q.enqueue(task2).unwrap();

        // Task 3: deadline in future (should remain)
        let mut task3 = make_task(3, Priority::Low, 300);
        task3.meta.deadline_ms = Some(2000);
        q.enqueue(task3).unwrap();

        // Task 4: deadline in past (should be pruned)
        let mut task4 = make_task(4, Priority::Critical, 400);
        task4.meta.deadline_ms = Some(800);
        q.enqueue(task4).unwrap();

        assert_eq!(q.len(), 4);

        // Prune at time 1000
        let pruned = q.prune_expired(1000).unwrap();
        let mut pruned_ids: Vec<_> = pruned.iter().map(|task| task.meta.id).collect();
        pruned_ids.sort_unstable();
        assert_eq!(pruned_ids, [2, 4]);
        assert!(q.prune_expired(1000).unwrap().is_empty());
        assert_eq!(q.len(), 2);

        // Remaining tasks should be 3 (low, deadline 2000) and 1 (normal, no deadline)
        // Task 1 has Normal priority, Task 3 has Low priority
        // So Task 1 should come out first
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 1);
        assert_eq!(q.dequeue().unwrap().unwrap().meta.id, 3);
    }

    #[test]
    fn test_empty_queue() {
        let mut q = InMemoryQueue::<String>::new(100);
        assert!(q.dequeue().unwrap().is_none());
        assert_eq!(q.len(), 0);
    }
}
