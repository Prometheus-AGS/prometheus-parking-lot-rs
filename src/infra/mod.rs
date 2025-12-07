//! Infrastructure adapters for queues, mailboxes, and storage backends.

pub mod mailbox;
pub mod queue;
pub use mailbox::InMemoryMailbox;
pub use mailbox::YaqueMailbox;
pub use queue::YaqueQueue;
pub use queue::InMemoryQueue;
