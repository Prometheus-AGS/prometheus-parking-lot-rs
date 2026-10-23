//! Shared helpers for the integration tests (included with `mod common;`).
#![allow(dead_code)]

use std::path::Path;

use parking_lot_ffi::{
    MailboxAddress, NewTask, ResourceAmount, ResourceClass, StoreConfig, TaskPriority,
};

pub fn config(path: &Path) -> StoreConfig {
    StoreConfig {
        path: path.to_string_lossy().into_owned(),
        max_depth: 100,
    }
}

pub fn task(id: u64, name: &str, priority: TaskPriority) -> NewTask {
    NewTask {
        task_id: id,
        payload: name.as_bytes().to_vec(),
        priority,
        cost: vec![ResourceAmount {
            resource: ResourceClass::Cpu,
            units: 1,
        }],
        idempotency_key: Some(name.to_owned()),
        mailbox: Some(mailbox()),
        deadline_ms: None,
    }
}

pub fn mailbox() -> MailboxAddress {
    MailboxAddress {
        tenant: "app".to_owned(),
        user_id: Some("user-1".to_owned()),
        session_id: None,
    }
}
