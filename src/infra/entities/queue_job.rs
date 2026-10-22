//! SeaORM entity for pl_queue_jobs table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Entity representing a queued task in the database.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "pl_queue_jobs")]
pub struct Model {
    /// Primary key.
    #[sea_orm(primary_key)]
    pub id: i64,

    /// Queue name for multi-pool support.
    pub queue_name: String,

    /// Task priority (0=Low, 1=Normal, 2=High, 3=Critical).
    pub priority: i16,

    /// Creation timestamp.
    pub created_at: DateTimeWithTimeZone,

    /// Creation timestamp in milliseconds since epoch.
    pub created_at_ms: i64,

    /// Deadline timestamp (optional).
    pub deadline_at: Option<DateTimeWithTimeZone>,

    /// Deadline in milliseconds since epoch (optional).
    pub deadline_ms: Option<i64>,

    /// Task metadata as JSON.
    pub metadata: Json,

    /// Task payload as JSON.
    pub payload: Json,

    /// Worker that locked this task (optional).
    pub locked_by: Option<String>,

    /// Lock acquisition timestamp (optional).
    pub locked_at: Option<DateTimeWithTimeZone>,

    /// Number of processing attempts.
    pub attempts: i32,
}

/// SeaORM relation stub — the queue_jobs table has no foreign-key relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
