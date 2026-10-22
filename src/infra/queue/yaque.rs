//! File-backed queue adapter inspired by Yaque.
//!
//! This is a simplified implementation using JSONL files to persist queued tasks.
//! It requires payloads to be serializable and deserializable.

use std::collections::VecDeque;
use std::fs::{OpenOptions, create_dir_all};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Serialize, de::DeserializeOwned};

use crate::core::{ScheduledTask, SchedulerError, TaskQueue};
/// File-backed queue using JSON lines for durability.
pub struct YaqueQueue<P> {
    path: PathBuf,
    stream: String,
    max_depth: usize,
    tasks: VecDeque<ScheduledTask<P>>,
}

impl<P> YaqueQueue<P> {
    /// Create a new Yaque-like queue.
    ///
    /// # Errors
    ///
    /// Returns a backend error when the directory cannot be created or an
    /// existing stream file cannot be read or parsed.
    pub fn new(
        path: impl AsRef<Path>,
        stream: impl Into<String>,
        max_depth: usize,
    ) -> Result<Self, SchedulerError>
    where
        P: DeserializeOwned,
    {
        let path = path.as_ref().to_path_buf();
        let stream = stream.into();
        create_dir_all(&path).map_err(|e| SchedulerError::backend("yaque::new", e))?;
        let mut queue = Self {
            path,
            stream,
            max_depth,
            tasks: VecDeque::new(),
        };
        queue.load_from_disk()?;
        Ok(queue)
    }

    fn file_path(&self) -> PathBuf {
        self.path.join(format!("{}.jsonl", self.stream))
    }

    fn load_from_disk(&mut self) -> Result<(), SchedulerError>
    where
        P: DeserializeOwned,
    {
        let file_path = self.file_path();
        if !file_path.exists() {
            return Ok(());
        }
        let file = OpenOptions::new()
            .read(true)
            .open(&file_path)
            .map_err(|e| SchedulerError::backend("yaque::load_from_disk::open", e))?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line =
                line.map_err(|e| SchedulerError::backend("yaque::load_from_disk::read", e))?;
            let task: ScheduledTask<P> = serde_json::from_str(&line)
                .map_err(|e| SchedulerError::backend("yaque::load_from_disk::deserialize", e))?;
            self.tasks.push_back(task);
        }
        Ok(())
    }

    fn append_to_disk(&self, task: &ScheduledTask<P>) -> Result<(), SchedulerError>
    where
        P: Serialize,
    {
        let file_path = self.file_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| SchedulerError::backend("yaque::append_to_disk::open", e))?;
        let line = serde_json::to_string(task)
            .map_err(|e| SchedulerError::backend("yaque::append_to_disk::serialize", e))?;
        writeln!(file, "{line}")
            .map_err(|e| SchedulerError::backend("yaque::append_to_disk::write", e))
    }

    fn rewrite_disk(&self, tasks: &VecDeque<ScheduledTask<P>>) -> Result<(), SchedulerError>
    where
        P: Serialize,
    {
        let file_path = self.file_path();
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&file_path)
            .map_err(|e| SchedulerError::backend("yaque::rewrite_disk::open", e))?;
        for task in tasks {
            let line = serde_json::to_string(task)
                .map_err(|e| SchedulerError::backend("yaque::rewrite_disk::serialize", e))?;
            writeln!(file, "{line}")
                .map_err(|e| SchedulerError::backend("yaque::rewrite_disk::write", e))?;
        }
        Ok(())
    }
}

impl<P> TaskQueue<P> for YaqueQueue<P>
where
    P: Serialize + DeserializeOwned + Clone,
{
    fn enqueue(&mut self, task: ScheduledTask<P>) -> Result<(), SchedulerError> {
        if self.len() >= self.max_depth() {
            return Err(SchedulerError::queue_full(
                "yaque",
                self.len(),
                self.max_depth,
            ));
        }
        self.tasks.push_back(task.clone());
        self.append_to_disk(&task)?;
        Ok(())
    }

    fn dequeue(&mut self) -> Result<Option<ScheduledTask<P>>, SchedulerError> {
        let item = self.tasks.pop_front();
        self.rewrite_disk(&self.tasks)?;
        Ok(item)
    }

    fn prune_expired(&mut self, now_ms: u128) -> Result<usize, SchedulerError> {
        let before = self.tasks.len();
        self.tasks
            .retain(|t| t.meta.deadline_ms.is_none_or(|d| d > now_ms));
        let after = self.tasks.len();
        self.rewrite_disk(&self.tasks)?;
        Ok(before.saturating_sub(after))
    }

    fn max_depth(&self) -> usize {
        self.max_depth
    }

    fn len(&self) -> usize {
        self.tasks.len()
    }
}
