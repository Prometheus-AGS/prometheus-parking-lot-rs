//! Integration tests for the Postgres backends (queue and mailbox).
//!
//! One test target (`--test integration`): Cargo auto-discovers
//! `tests/integration/main.rs` and the modules below are its siblings (cr-35).
//!
//! These need Docker, so every test is `#[ignore]`d; they compile in CI and run
//! with:
//!
//! ```text
//! cargo test -p prometheus-parking-lot-pg --test integration -- --ignored
//! ```
//!
//! Containers come from the cr-43 helper (`tests/support`), which builds or
//! reuses the `parking-lot-pg:<major>` image.

use sea_orm::{Database, DatabaseConnection};

#[path = "../support/mod.rs"]
mod support;

mod postgres_mailbox_test;
mod postgres_queue_test;

use support::{PgContainer, TestResult};

/// Postgres major version the suites run against. The 17/18 matrix is cr-36.
const PG_MAJOR: u8 = 18;

/// Runs a synchronous backend call (the `TaskQueue` and `Mailbox` contracts are
/// synchronous and bridge onto async `SeaORM` with `Handle::block_on`) from an
/// async test without panicking inside the runtime. Tests using it must be
/// `flavor = "multi_thread"`.
macro_rules! blocking {
    ($call:expr) => {
        ::tokio::task::block_in_place(|| $call)
    };
}
pub(crate) use blocking;

/// A running Postgres container with the queue and mailbox tables created.
pub(crate) struct PostgresTestFixture {
    // Held so the container is removed on drop.
    _pg: PgContainer,
    pub(crate) db: DatabaseConnection,
}

impl PostgresTestFixture {
    /// Start a container, run the table migrations and open a `SeaORM` connection.
    pub(crate) async fn new() -> TestResult<Self> {
        let pg = PgContainer::start(PG_MAJOR).await?;

        for migration in [
            include_str!("../../migrations/001_create_queue_table.sql"),
            include_str!("../../migrations/002_create_mailbox_table.sql"),
        ] {
            sqlx::raw_sql(migration).execute(&pg.pool).await?;
        }

        let db = Database::connect(pg.url.as_str()).await?;
        Ok(Self { _pg: pg, db })
    }
}

/// Test payload for queue and mailbox tests.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub(crate) struct TestPayload {
    pub(crate) id: u64,
    pub(crate) data: String,
}

impl TestPayload {
    pub(crate) fn new(id: u64, data: impl Into<String>) -> Self {
        Self {
            id,
            data: data.into(),
        }
    }
}

/// Await every handle in order (a stand-in for `futures::future::join_all`).
pub(crate) async fn join_all<T>(
    handles: Vec<tokio::task::JoinHandle<T>>,
) -> Vec<Result<T, tokio::task::JoinError>> {
    let mut out = Vec::with_capacity(handles.len());
    for handle in handles {
        out.push(handle.await);
    }
    out
}
