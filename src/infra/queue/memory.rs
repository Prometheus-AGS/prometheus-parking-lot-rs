//! In-memory queue with priority and deadline awareness.

use std::cmp::Ordering;
use std::collections::VecDeque;

use crate::core::SchedulerError;
use crate::core::{ScheduledTask, TaskQueue};
use crate::util::serde::Priority;

/// In-memory queue storing scheduled tasks.
pub struct InMemoryQueue<P> {
    max_depth: usize,
    tasks: VecDeque<ScheduledTask<P>>,
}

impl<P> InMemoryQueue<P> {
    /// Create a new in-memory queue with a maximum depth.
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            tasks: VecDeque::new(),
        }
    }

    fn priority_value(p: Priority) -> u8 {
        match p {
            Priority::Low => 0,
            Priority::Normal => 1,
            Priority::High => 2,
            Priority::Critical => 3,
        }
    }

    fn sort_tasks(tasks: &mut VecDeque<ScheduledTask<P>>) {
        let mut vec: Vec<_> = tasks.drain(..).collect();
        vec.sort_by(|a, b| {
            let pa = Self::priority_value(a.meta.priority);
            let pb = Self::priority_value(b.meta.priority);
            match pb.cmp(&pa) {
                Ordering::Equal => a.meta.created_at_ms.cmp(&b.meta.created_at_ms),
                other => other,
            }
        });
        tasks.extend(vec);
    }
}

impl<P> TaskQueue<P> for InMemoryQueue<P> {
    fn enqueue(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError> {
        if self.len() >= self.max_depth() {
            return Err(SchedulerError::QueueFull("max queue depth reached".into()));
        }
        self.tasks.push_back(task);
        Self::sort_tasks(&mut self.tasks);
        Ok(())
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError> {
        Ok(self.tasks.pop_front())
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        let before = self.tasks.len();
        self.tasks
            .retain(|t| t.meta.deadline_ms.map(|d| d > now_ms).unwrap_or(true));
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
