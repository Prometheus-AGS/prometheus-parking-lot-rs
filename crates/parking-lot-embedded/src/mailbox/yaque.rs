//! File-backed mailbox adapter inspired by Yaque.

use std::collections::HashMap;
use std::fs::{OpenOptions, create_dir_all};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use prometheus_parking_lot_core::MailboxKey;
use prometheus_parking_lot_core::now_ms;
use prometheus_parking_lot_core::{Mailbox, SchedulerError, TaskStatus};

/// File-backed mailbox using JSON lines for durability.
pub struct YaqueMailbox<P> {
    path: PathBuf,
    stream: String,
    messages: HashMap<MailboxKey, Vec<MailboxMessage<P>>>,
}

/// Mailbox message container.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage<P> {
    /// Task status.
    pub status: TaskStatus,
    /// Optional payload.
    pub payload: Option<P>,
    /// Timestamp milliseconds.
    pub created_at_ms: u128,
}

impl<P> YaqueMailbox<P> {
    /// Create a new mailbox persisted to the given path/stream.
    ///
    /// # Errors
    ///
    /// Returns a backend error when the directory cannot be created or an
    /// existing stream file cannot be read or parsed.
    pub fn new(path: impl AsRef<Path>, stream: impl Into<String>) -> Result<Self, SchedulerError>
    where
        P: DeserializeOwned,
    {
        let path = path.as_ref().to_path_buf();
        let stream = stream.into();
        create_dir_all(&path).map_err(|e| SchedulerError::backend("yaque_mailbox", e))?;
        let mut mb = Self {
            path,
            stream,
            messages: HashMap::new(),
        };
        mb.load_from_disk()?;
        Ok(mb)
    }

    fn file_path(&self) -> PathBuf {
        self.path.join(format!("{}_mailbox.jsonl", self.stream))
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
            .map_err(|e| SchedulerError::backend("yaque_mailbox", e))?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line.map_err(|e| SchedulerError::backend("yaque_mailbox", e))?;
            let (key, msg): (MailboxKey, MailboxMessage<P>) = serde_json::from_str(&line)
                .map_err(|e| SchedulerError::backend("yaque_mailbox", e))?;
            self.messages.entry(key).or_default().push(msg);
        }
        Ok(())
    }

    fn append_to_disk(
        &self,
        key: &MailboxKey,
        msg: &MailboxMessage<P>,
    ) -> Result<(), SchedulerError>
    where
        P: Serialize,
    {
        let file_path = self.file_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
            .map_err(|e| SchedulerError::backend("yaque_mailbox", e))?;
        let line = serde_json::to_string(&(key, msg))
            .map_err(|e| SchedulerError::backend("yaque_mailbox", e))?;
        writeln!(file, "{line}").map_err(|e| SchedulerError::backend("yaque_mailbox", e))
    }

    /// Fetch messages for a mailbox key, optionally since a timestamp.
    #[must_use]
    pub fn fetch(
        &self,
        key: &MailboxKey,
        since_ms: Option<u128>,
        limit: usize,
    ) -> Vec<MailboxMessage<P>>
    where
        P: Clone,
    {
        self.messages
            .get(key)
            .map(|msgs| {
                msgs.iter()
                    .filter(|m| since_ms.is_none_or(|s| m.created_at_ms >= s))
                    .take(limit)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl<P> Mailbox<P> for YaqueMailbox<P>
where
    P: Serialize + DeserializeOwned + Clone,
{
    fn deliver(
        &mut self,
        key: &MailboxKey,
        status: TaskStatus,
        payload: Option<P>,
    ) -> Result<(), SchedulerError> {
        let msg = MailboxMessage {
            status,
            payload,
            created_at_ms: now_ms(),
        };
        self.messages
            .entry(key.clone())
            .or_default()
            .push(msg.clone());
        self.append_to_disk(key, &msg)
    }
}
