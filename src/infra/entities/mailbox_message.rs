//! SeaORM entity for pl_mailbox_messages table.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

/// Entity representing a mailbox message (task result) in the database.
#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "pl_mailbox_messages")]
pub struct Model {
    /// Primary key.
    #[sea_orm(primary_key)]
    pub id: i64,
    
    /// Tenant identifier.
    pub tenant: String,
    
    /// User identifier (optional).
    pub user_id: Option<String>,
    
    /// Session identifier (optional).
    pub session_id: Option<String>,
    
    /// Task ID (optional).
    pub task_id: Option<i64>,
    
    /// Creation timestamp.
    pub created_at: DateTimeWithTimeZone,
    
    /// Creation timestamp in milliseconds since epoch.
    pub created_at_ms: i64,
    
    /// Task status (Queued, Running, Completed, Failed, Expired, Dropped).
    pub status: String,
    
    /// Task result payload as JSON (optional).
    pub payload: Option<Json>,
}

/// SeaORM relation stub — the mailbox_messages table has no foreign-key relations.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
