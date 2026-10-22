//! Infrastructure adapters for queues, mailboxes, and storage backends.

pub mod mailbox;
pub mod queue;

#[cfg(feature = "postgres")]
pub mod entities;

pub use mailbox::InMemoryMailbox;
pub use mailbox::YaqueMailbox;
pub use queue::InMemoryQueue;
pub use queue::YaqueQueue;
