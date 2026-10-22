//! `SeaORM` entity models for Postgres backend.
//!
//! These entities map to the database tables defined in the migration scripts.

#[cfg(feature = "postgres")]
pub mod queue_job;

#[cfg(feature = "postgres")]
pub mod mailbox_message;

#[cfg(feature = "postgres")]
pub use queue_job::Entity as QueueJob;

#[cfg(feature = "postgres")]
pub use mailbox_message::Entity as MailboxMessage;
