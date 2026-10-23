//! Mailbox implementations that need no IO, and the composition of a mailbox
//! with a notifier ([`StorageMailbox`](crate::contracts::StorageMailbox)).

pub mod memory;
pub mod notify;
mod storage;
#[cfg(test)]
mod storage_tests;

pub use memory::InMemoryMailbox;
pub use notify::{
    BackgroundNotifier, DetachedNotifier, NotifyStats, NotifyStatsSnapshot, RetryPolicy, Sleeper,
};
