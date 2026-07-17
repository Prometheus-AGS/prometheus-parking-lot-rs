//! Integration test infrastructure for Postgres backends.
//!
//! This module provides test utilities for running integration tests against
//! a real Postgres database using testcontainers.

#![cfg(feature = "postgres")]

use sea_orm::{Database, DatabaseConnection};
use std::time::Duration;
use testcontainers::{clients::Cli, core::WaitFor, Container, Image, RunnableImage};
use testcontainers_modules::postgres::Postgres;

pub mod postgres_queue_test;
pub mod postgres_mailbox_test;

/// Custom Postgres image with our initialization SQL.
#[derive(Debug, Default, Clone)]
pub struct CustomPostgres {
    inner: Postgres,
}

impl Image for CustomPostgres {
    type Args = <Postgres as Image>::Args;

    fn name(&self) -> String {
        self.inner.name()
    }

    fn tag(&self) -> String {
        self.inner.tag()
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        self.inner.ready_conditions()
    }

    fn env_vars(&self) -> Box<dyn Iterator<Item = (&String, &String)> + '_> {
        self.inner.env_vars()
    }
}

/// Test fixture that manages a Postgres container and database connection.
pub struct PostgresTestFixture<'a> {
    _container: Container<'a, CustomPostgres>,
    pub db: DatabaseConnection,
}

impl<'a> PostgresTestFixture<'a> {
    /// Create a new test fixture with a running Postgres container.
    pub async fn new(docker: &'a Cli) -> Result<Self, Box<dyn std::error::Error>> {
        // Start Postgres container
        let postgres_image = CustomPostgres::default();
        let runnable = RunnableImage::from(postgres_image)
            .with_tag("17-alpine");
        
        let container = docker.run(runnable);
        
        // Wait a bit for container to be fully ready
        tokio::time::sleep(Duration::from_secs(2)).await;

        // Get connection info
        let port = container.get_host_port_ipv4(5432);
        let connection_string = format!(
            "postgresql://postgres:postgres@127.0.0.1:{}/postgres",
            port
        );

        // Connect to database
        let db = Database::connect(&connection_string).await?;

        // Run migrations
        Self::run_migrations(&db).await?;

        Ok(Self {
            _container: container,
            db,
        })
    }

    /// Run migration SQL to create tables.
    async fn run_migrations(db: &DatabaseConnection) -> Result<(), Box<dyn std::error::Error>> {
        use sea_orm::Statement;

        // Create queue table
        let queue_migration = include_str!("../../migrations/001_create_queue_table.sql");
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            queue_migration.to_string(),
        ))
        .await?;

        // Create mailbox table
        let mailbox_migration = include_str!("../../migrations/002_create_mailbox_table.sql");
        db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            mailbox_migration.to_string(),
        ))
        .await?;

        Ok(())
    }

    /// Clean up all test data from tables.
    pub async fn cleanup(&self) -> Result<(), Box<dyn std::error::Error>> {
        use sea_orm::Statement;

        self.db.execute(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "TRUNCATE TABLE pl_queue_jobs, pl_mailbox_messages RESTART IDENTITY".to_string(),
        ))
        .await?;

        Ok(())
    }
}

/// Helper to create a test payload for queue tests.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct TestPayload {
    pub id: u64,
    pub data: String,
}

impl TestPayload {
    pub fn new(id: u64, data: impl Into<String>) -> Self {
        Self {
            id,
            data: data.into(),
        }
    }
}
