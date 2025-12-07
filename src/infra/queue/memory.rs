//! In-memory queue with priority and deadline awareness.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use crate::core::SchedulerError;
use crate::core::{ScheduledTask, TaskQueue};
use crate::util::serde::Priority;

/// Wrapper to make ScheduledTask orderable by priority (highest first) and FIFO within priority.
struct PriorityTask<P> {
    task: ScheduledTask<P>,
}

impl<P> PriorityTask<P> {
    fn priority_value(p: Priority) -> u8 {
        match p {
            Priority::Low => 0,
            Priority::Normal => 1,
            Priority::High => 2,
            Priority::Critical => 3,
        }
    }
}

impl<P> PartialEq for PriorityTask<P> {
    fn eq(&self, other: &Self) -> bool {
        self.task.meta.id == other.task.meta.id
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
        let self_priority = Self::priority_value(self.task.meta.priority);
        let other_priority = Self::priority_value(other.task.meta.priority);
        
        // Higher priority first
        match self_priority.cmp(&other_priority) {
            Ordering::Equal => {
                // FIFO within same priority: earlier created_at wins (reversed for max-heap)
                other.task.meta.created_at_ms.cmp(&self.task.meta.created_at_ms)
            }
            other => other,
        }
    }
}

/// In-memory queue storing scheduled tasks using a priority heap.
/// This provides O(log n) enqueue and O(log n) dequeue operations.
pub struct InMemoryQueue<P> {
    max_depth: usize,
    /// Binary heap for O(log n) priority-based operations.
    tasks: BinaryHeap<PriorityTask<P>>,
}

impl<P> InMemoryQueue<P> {
    /// Create a new in-memory queue with a maximum depth.
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
            return Err(SchedulerError::QueueFull("max queue depth reached".into()));
        }
        // O(log n) insertion
        self.tasks.push(PriorityTask { task });
        Ok(())
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError> {
        // O(log n) removal
        Ok(self.tasks.pop().map(|pt| pt.task))
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        let before = self.tasks.len();
        // Rebuild heap without expired tasks
        let tasks: Vec<_> = self.tasks.drain().collect();
        self.tasks = tasks
            .into_iter()
            .filter(|pt| pt.task.meta.deadline_ms.map(|d| d > now_ms).unwrap_or(true))
            .collect();
        let after = self.tasks.len();
        Ok(before.saturating_sub(after))
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
    use crate::util::serde::{ResourceCost, ResourceKind};

    fn make_task(id: u64, priority: Priority, created_at_ms: u128) -> ScheduledTask<String> {
        ScheduledTask {
            meta: crate::core::TaskMetadata {
                id,
                mailbox: None,
                priority,
                cost: ResourceCost {
                    kind: ResourceKind::Cpu,
                    units: 1,
                },
                deadline_ms: None,
                created_at_ms,
            },
            payload: format!("task-{}", id),
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
        assert_eq!(pruned, 2); // Tasks 2 and 4 expired
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
