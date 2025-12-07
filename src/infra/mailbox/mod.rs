//! Mailbox backends.

pub mod memory;
pub mod postgres;
pub mod yaque;

pub use memory::InMemoryMailbox;
pub use postgres::PostgresMailbox;
pub use yaque::YaqueMailbox;
