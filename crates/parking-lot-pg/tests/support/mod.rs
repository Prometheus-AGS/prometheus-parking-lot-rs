//! Testcontainers helper for the Postgres substrate (CR-43).
//!
//! [`PgContainer::start`] builds-or-reuses the `parking-lot-pg:<major>` image
//! (Postgres + pgmq, see `docker/postgres18` and `docker/postgres17`), starts a
//! fresh container and returns a connected [`sqlx::PgPool`] plus the URL.
//!
//! The image is built once per machine: if `docker image inspect` finds the tag
//! it is reused; set `PARKING_LOT_PG_REBUILD=1` to force a rebuild after editing
//! the Dockerfile. Every container is removed when the guard is dropped.
//! OA-14's run-bound upgraded URL instead selects fresh per-test databases on
//! that upgraded server; the rehearsal runner owns its container and volumes.

#![allow(
    dead_code,
    reason = "shared by several test binaries; each uses a subset"
)]

use std::{error::Error, path::PathBuf, process::Command, time::Duration};

use sqlx::{PgPool, postgres::PgPoolOptions};
use testcontainers::{
    ContainerAsync, GenericImage,
    core::{ContainerPort, WaitFor},
    runners::AsyncRunner,
};

/// Boxed error used by the helper; tests just propagate it with `?`.
pub type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

/// Superuser name configured in the container.
pub const PG_USER: &str = "postgres";
/// Superuser password configured in the container (test-only credential).
pub const PG_PASSWORD: &str = "postgres";
/// Default database created by the entrypoint.
pub const PG_DATABASE: &str = "postgres";

/// Postgres major version under test: `PARKING_LOT_PG_MAJOR` (17 or 18), default 18.
///
/// The CI matrix (`.github/workflows/ci.yml`) sets it per leg. A value
/// that is set but unparsable panics instead of silently testing the default,
/// so a typo cannot turn the 17 leg into a second 18 run.
///
/// # Panics
///
/// If `PARKING_LOT_PG_MAJOR` is set to anything but a supported major.
#[must_use]
pub fn pg_major() -> u8 {
    std::env::var("PARKING_LOT_PG_MAJOR").map_or(18, |v| match v.trim().parse::<u8>() {
        Ok(major @ (17 | 18)) => major,
        _ => panic!("PARKING_LOT_PG_MAJOR must be 17 or 18, got {v:?}"),
    })
}

/// A running `parking-lot-pg` container with a ready pool.
pub struct PgContainer {
    /// Pool connected to the default database.
    pub pool: PgPool,
    /// `postgres://` URL for the default database (host-mapped port).
    pub url: String,
    /// Postgres major version of the image (17 or 18).
    pub major: u8,
    // Held so the container is removed on drop.
    _container: Option<ContainerAsync<GenericImage>>,
}

impl PgContainer {
    /// Image tag for a Postgres major version, e.g. `parking-lot-pg:18`.
    #[must_use]
    pub fn image_tag(major: u8) -> String {
        format!("parking-lot-pg:{major}")
    }

    /// Build-or-reuse the image, start a container and return a ready pool.
    ///
    /// # Errors
    ///
    /// Returns an error if Docker is unavailable, the image build fails, the
    /// container does not start, or Postgres does not accept connections
    /// within 60 seconds.
    pub async fn start(major: u8) -> TestResult<Self> {
        if let Ok(url) = std::env::var("PARKING_LOT_PG_UPGRADE_URL") {
            return Self::on_upgraded_cluster(major, &url).await;
        }
        ensure_image(major).await?;

        let container = GenericImage::new("parking-lot-pg", &major.to_string())
            .with_exposed_port(ContainerPort::Tcp(5432))
            .with_wait_for(WaitFor::message_on_stderr(
                "database system is ready to accept connections",
            ));
        let container =
            testcontainers::ImageExt::with_env_var(container, "POSTGRES_PASSWORD", PG_PASSWORD)
                .start()
                .await?;

        let host = container.get_host().await?;
        let port = container.get_host_port_ipv4(5432).await?;
        let url = format!("postgres://{PG_USER}:{PG_PASSWORD}@{host}:{port}/{PG_DATABASE}");
        let pool = connect_with_retry(&url).await?;
        Ok(Self {
            pool,
            url,
            major,
            _container: Some(container),
        })
    }

    /// OA-14 reuses the upgraded cluster, but each ordinary suite still owns a
    /// fresh database. A run-bound marker is checked before CREATE DATABASE;
    /// this path cannot silently select an arbitrary operator database.
    async fn on_upgraded_cluster(major: u8, url: &str) -> TestResult<Self> {
        if major != 18 {
            return Err("OA-14 upgraded-cluster suites require PARKING_LOT_PG_MAJOR=18".into());
        }
        let run_id = std::env::var("PARKING_LOT_PG_UPGRADE_RUN_ID")?;
        let admin = connect_with_retry(url).await?;
        let owned: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM oa14_rehearsal_identity \
             WHERE run_id = $1 AND fixture_id = 'oa14-populated-0.3-pg17-v1') \
             AND current_setting('server_version_num')::int / 10000 = 18",
        )
        .bind(run_id)
        .fetch_one(&admin)
        .await?;
        if !owned {
            return Err("OA-14 unsafe upgraded-cluster identity".into());
        }
        let database = database_name()?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE DATABASE {database} TEMPLATE template1"
        )))
        .execute(&admin)
        .await?;
        admin.close().await;
        let (prefix, _) = url.rsplit_once('/').ok_or("OA-14 URL has no database")?;
        let url = format!("{prefix}/{database}");
        let pool = connect_with_retry(&url).await?;
        Ok(Self {
            pool,
            url,
            major,
            _container: None,
        })
    }

    /// URL for a different database on the same server (create it first).
    #[must_use]
    pub fn url_for(&self, database: &str) -> String {
        match self.url.rsplit_once('/') {
            Some((prefix, _)) => format!("{prefix}/{database}"),
            None => self.url.clone(),
        }
    }
}

/// An owned SQL identifier, also used by backup/child database scenarios when
/// many otherwise isolated tests share the OA-14 upgraded server.
pub fn database_name() -> TestResult<String> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    Ok(format!("oa14_{}_{stamp}", std::process::id()))
}

/// The official entrypoint logs "ready" for its temporary init server too, so
/// readiness is confirmed by actually connecting.
async fn connect_with_retry(url: &str) -> TestResult<PgPool> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        match PgPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await
        {
            Ok(pool) => return Ok(pool),
            Err(err) if tokio::time::Instant::now() >= deadline => return Err(err.into()),
            Err(_) => tokio::time::sleep(Duration::from_millis(250)).await,
        }
    }
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

async fn ensure_image(major: u8) -> TestResult<()> {
    tokio::task::spawn_blocking(move || build_if_missing(major)).await?
}

fn build_if_missing(major: u8) -> TestResult<()> {
    let tag = PgContainer::image_tag(major);
    let rebuild = std::env::var_os("PARKING_LOT_PG_REBUILD").is_some();
    let present = Command::new("docker")
        .args(["image", "inspect", &tag])
        .output()?
        .status
        .success();
    if present && !rebuild {
        return Ok(());
    }
    let root = workspace_root();
    let dockerfile = root.join(format!("docker/postgres{major}/Dockerfile"));
    let status = Command::new("docker")
        .arg("build")
        .arg("-f")
        .arg(&dockerfile)
        .args(["-t", &tag])
        .arg(root.join("docker"))
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("docker build of {tag} failed ({status})").into())
    }
}
