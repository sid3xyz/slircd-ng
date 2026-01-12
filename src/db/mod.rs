//! Database module for persistent storage.
//!
//! Provides async SQLite database access using SQLx for:
//! - NickServ accounts and nicknames
//! - ChanServ channel registration and access lists
//! - K-lines and D-lines persistence
//! - Message history for CHATHISTORY

mod accounts;
mod bans;
mod channels;

pub use accounts::AccountRepository;
pub use bans::{BanRepository, Dline, Gline, Kline, Shun, Zline};
pub use channels::{ChannelAkick, ChannelRecord, ChannelRepository};

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tracing::info;

/// Database errors.
#[derive(Debug, Error)]
pub enum DbError {
    #[error("database error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
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
            // Use file::memory: with shared cache for connection pool compatibility
            let options = SqliteConnectOptions::new()
                .filename("file::memory:")
                .shared_cache(true)
                .create_if_missing(true);

            SqlitePoolOptions::new()
                .max_connections(5)
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

        Ok(Self { pool })
    }

    /// Get reference to the underlying connection pool.
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Run embedded migrations.
    /// Checks for each table and runs the full migration if any are missing.
    async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
        async fn table_exists(pool: &SqlitePool, table: &str) -> bool {
            sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?)",
            )
            .bind(table)
            .fetch_one(pool)
            .await
            .unwrap_or(false)
        }

        async fn column_exists(pool: &SqlitePool, table: &str, column: &str) -> bool {
            // pragma_table_info() returns one row per column.
            // We check for the presence of the column name.
            let sql = format!(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info('{}') WHERE name=?)",
                table.replace('"', "")
            );
            sqlx::query_scalar::<_, bool>(&sql)
                .bind(column)
                .fetch_one(pool)
                .await
                .unwrap_or(false)
        }

        // 001_init.sql: core schema (accounts/channels/basics).
        let core_tables = [
            "accounts",
            "nicknames",
            "klines",
            "dlines",
            "channels",
            "channel_access",
            "channel_akick",
        ];
        let mut core_ok = true;
        for t in core_tables {
            if !table_exists(pool, t).await {
                core_ok = false;
                break;
            }
        }

        if !core_ok {
            Self::run_migration_file(pool, include_str!("../../migrations/001_init.sql")).await;
            info!("Database migrations applied (001_init)");
        }

        // 002_shuns.sql: shuns table.
        if !table_exists(pool, "shuns").await {
            Self::run_migration_file(pool, include_str!("../../migrations/002_shuns.sql")).await;
            info!("Database migrations applied (002_shuns)");
        }

        // 002_xlines.sql: extended bans (G/Z/R-lines) + expiry indexes.
        let xline_tables = ["glines", "zlines", "rlines"];
        let mut xlines_ok = true;
        for t in xline_tables {
            if !table_exists(pool, t).await {
                xlines_ok = false;
                break;
            }
        }
        if !xlines_ok {
            Self::run_migration_file(pool, include_str!("../../migrations/002_xlines.sql")).await;
            info!("Database migrations applied (002_xlines)");
        }

        // 003_history.sql: message history (IF NOT EXISTS, but we still gate for log cleanliness).
        if !table_exists(pool, "message_history").await {
            Self::run_migration_file(pool, include_str!("../../migrations/003_history.sql")).await;
            info!("Database migrations applied (003_history)");
        }

        // 004_certfp.sql: adds accounts.certfp column (must not run twice).
        if table_exists(pool, "accounts").await && !column_exists(pool, "accounts", "certfp").await
        {
            Self::run_migration_file(pool, include_str!("../../migrations/004_certfp.sql")).await;
            info!("Database migrations applied (004_certfp)");
        }

        // 005_channel_topics.sql: adds topic columns to channels table.
        if table_exists(pool, "channels").await
            && !column_exists(pool, "channels", "topic_text").await
        {
            Self::run_migration_file(
                pool,
                include_str!("../../migrations/005_channel_topics.sql"),
            )
            .await;
            info!("Database migrations applied (005_channel_topics)");
        }

        // 006_reputation.sql: reputation tracking table.
        if !table_exists(pool, "reputation").await {
            Self::run_migration_file(pool, include_str!("../../migrations/006_reputation.sql"))
                .await;
            info!("Database migrations applied (006_reputation)");
        }

        // 007_scram_verifiers.sql: SCRAM-SHA-256 columns for SASL authentication.
        if !column_exists(pool, "accounts", "scram_salt").await {
            Self::run_migration_file(
                pool,
                include_str!("../../migrations/007_scram_verifiers.sql"),
            )
            .await;
            info!("Database migrations applied (007_scram_verifiers)");
        }

        // Best-effort informational log.
        if core_ok
            && table_exists(pool, "shuns").await
            && table_exists(pool, "glines").await
            && table_exists(pool, "zlines").await
            && table_exists(pool, "rlines").await
            && table_exists(pool, "message_history").await
            && column_exists(pool, "accounts", "certfp").await
            && column_exists(pool, "channels", "topic_text").await
            && table_exists(pool, "reputation").await
            && column_exists(pool, "accounts", "scram_salt").await
        {
            info!("Database already initialized");
        }

        Ok(())
    }

    /// Run a single migration file, executing each statement.
    async fn run_migration_file(pool: &SqlitePool, migration: &str) {
        for statement in migration.split(';') {
            // Remove leading comments and whitespace to get actual SQL
            let mut sql_lines: Vec<&str> = Vec::new();
            for line in statement.lines() {
                let line = line.trim();
                // Skip empty lines and comment-only lines
                if line.is_empty() || line.starts_with("--") {
                    continue;
                }
                sql_lines.push(line);
            }

            if sql_lines.is_empty() {
                continue;
            }

            // Rejoin the SQL statement
            let sql = sql_lines.join("\n");

            // Execute each statement, logging errors
            if let Err(e) = sqlx::query(&sql).execute(pool).await {
                // Only log if it's not a "table already exists" error
                let err_str = e.to_string();
                if !err_str.contains("already exists") {
                    tracing::warn!(sql = %sql, error = %e, "Migration statement failed");
                }
            }
        }
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
