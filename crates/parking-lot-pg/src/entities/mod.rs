//! `SeaORM` entity models for Postgres backend.
//!
//! These entities map to the database tables defined in the migration scripts.

pub mod queue_job;

pub mod mailbox_message;

pub use queue_job::Entity as QueueJob;

pub use mailbox_message::Entity as MailboxMessage;
