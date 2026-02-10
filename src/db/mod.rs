//! Database module for persistent storage.
//!
//! Provides async SQLite database access using SQLx for:
//! - NickServ accounts and nicknames
//! - ChanServ channel registration and access lists
//! - K-lines and D-lines persistence
//! - Message history for CHATHISTORY
//!
//! Also provides Redb-backed persistence for:
//! - Always-on client state (bouncer functionality)

mod accounts;
pub mod always_on;
mod bans;
mod channels;

pub use accounts::AccountRepository;
pub use always_on::{AlwaysOnError, AlwaysOnStore};
pub use bans::{BanRepository, Dline, Gline, Kline, Shun, Zline};
pub use channels::{ChannelAkick, ChannelRecord, ChannelRepository};

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use thiserror::Error;
use tracing::info;

static MEMDB_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Database errors.
#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(sqlx::Error),
    #[error("migration error: {0}")]
    Migration(sqlx::migrate::MigrateError),
    #[error("account not found: {0}")]
    AccountNotFound(String),
    #[error("nickname not found: {0}")]
    NicknameNotFound(String),
    #[error("account already exists: {0}")]
    AccountExists(String),
    #[error("nickname already registered: {0}")]
    NicknameRegistered(String),
    #[error("invalid password")]
    InvalidPassword,
    #[error("unknown option: {0}")]
    UnknownOption(String),
    #[error("channel already registered: {0}")]
    ChannelExists(String),
    #[error("insufficient access")]
    InsufficientAccess,
    #[error("internal error: {0}")]
    Internal(String),
}

/// Database handle with connection pool.
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Connection acquire timeout - prevents connection storms from blocking indefinitely.
    const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(5);

    /// Maximum time a connection can remain idle before being closed.
    const IDLE_TIMEOUT: Duration = Duration::from_secs(60);

    /// Create a new database connection, running migrations if needed.
    pub async fn new(path: &str) -> Result<Self, DbError> {
        let pool = if path == ":memory:" {
            // In-memory database - use proper SQLx in-memory mode
            // Use a uniquely named shared-cache memory database per call.
            // `file::memory:` is global-ish and will collide across parallel tests.
            let id = MEMDB_COUNTER.fetch_add(1, Ordering::Relaxed);
            let memdb_uri = format!(
                "file:slircd-ng-memdb-{}-{}?mode=memory&cache=shared",
                std::process::id(),
                id
            );

            let options = SqliteConnectOptions::new()
                .filename(&memdb_uri)
                .shared_cache(true)
                .create_if_missing(true);

            SqlitePoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Self::ACQUIRE_TIMEOUT)
                .idle_timeout(Some(Self::IDLE_TIMEOUT))
                .test_before_acquire(true)
                .connect_with(options)
                .await?
        } else {
            // File-based database
            // Create parent directory if it doesn't exist
            if let Some(parent) = Path::new(path).parent()
                && !parent.as_os_str().is_empty()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                tracing::warn!(path = %parent.display(), error = %e, "Failed to create database directory");
            }

            let options = SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true);

            SqlitePoolOptions::new()
                .max_connections(5)
                .acquire_timeout(Self::ACQUIRE_TIMEOUT)
                .idle_timeout(Some(Self::IDLE_TIMEOUT))
                .test_before_acquire(true)
                .connect_with(options)
                .await?
        };

        info!(path = %path, "Database connected");

        // Run embedded migrations
        Self::run_migrations(&pool).await?;

        // Enable WAL mode for better concurrency (reduces lock contention)
        // WAL mode allows reads to happen while writes are in progress
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await?;

        // Enable foreign key constraints (critical for ON DELETE CASCADE schema)
        sqlx::query("PRAGMA foreign_keys=ON").execute(&pool).await?;

        // Use NORMAL synchronous mode instead of FULL for better performance
        // NORMAL provides good durability while being faster than FULL
        // (trades immediate disk fsync for transaction durability)
        sqlx::query("PRAGMA synchronous=NORMAL")
            .execute(&pool)
            .await?;

        // Check database integrity on startup (prevents silent corruption from crashes)
        let integrity_result: String = sqlx::query_scalar("PRAGMA integrity_check")
            .fetch_one(&pool)
            .await?;

        if integrity_result != "ok" {
            tracing::error!(
                integrity_check = %integrity_result,
                "Database integrity check FAILED - corruption detected!"
            );
            return Err(DbError::Sqlx(sqlx::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Database integrity check failed: {}", integrity_result),
            ))));
        }

        info!("Database integrity check passed");

        Ok(Self { pool })
    }

    /// Get reference to the underlying connection pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Run embedded migrations.
    async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
        sqlx::migrate!("./migrations")
            .run(pool)
            .await
            .map_err(DbError::Migration)?;

        info!("Database migrations checked/applied");
        Ok(())
    }

    /// Get account repository.
    pub fn accounts(&self) -> AccountRepository<'_> {
        AccountRepository::new(&self.pool)
    }

    /// Get channel repository.
    pub fn channels(&self) -> ChannelRepository<'_> {
        ChannelRepository::new(&self.pool)
    }

    /// Get ban repository.
    pub fn bans(&self) -> BanRepository<'_> {
        BanRepository::new(&self.pool)
    }
}

impl From<sqlx::Error> for DbError {
    fn from(err: sqlx::Error) -> Self {
        DbError::Sqlx(err)
    }
}

impl From<sqlx::migrate::MigrateError> for DbError {
    fn from(err: sqlx::migrate::MigrateError) -> Self {
        DbError::Migration(err)
    }
}
