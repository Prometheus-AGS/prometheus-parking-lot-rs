//! Mailbox implementations that need no IO.

pub mod memory;

pub use memory::InMemoryMailbox;
