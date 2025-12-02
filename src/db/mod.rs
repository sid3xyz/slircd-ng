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
mod history;

pub use accounts::AccountRepository;
pub use bans::{BanRepository, Dline, Gline, Kline, Shun, Zline};
pub use channels::{ChannelAkick, ChannelRecord, ChannelRepository};
pub use history::{HistoryRepository, StoredMessage, StoreMessageParams};

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use std::path::Path;
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
    #[error("channel not found: {0}")]
    #[allow(dead_code)]
    ChannelNotFound(String),
    #[error("channel already registered: {0}")]
    ChannelExists(String),
    #[error("not channel founder")]
    #[allow(dead_code)] // Future: channel ownership checks
    NotFounder,
    #[error("insufficient access")]
    InsufficientAccess,
}

/// Database handle with connection pool.
#[derive(Clone)]
pub struct Database {
    pool: SqlitePool,
}

impl Database {
    /// Create a new database connection, running migrations if needed.
    pub async fn new(path: &str) -> Result<Self, DbError> {
        // Create parent directory if it doesn't exist
        if let Some(parent) = Path::new(path).parent()
            && !parent.as_os_str().is_empty()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(path = %parent.display(), error = %e, "Failed to create database directory");
        }

        // Configure SQLite connection
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;

        info!(path = %path, "Database connected");

        // Run embedded migrations
        Self::run_migrations(&pool).await?;

        Ok(Self { pool })
    }

    /// Run embedded migrations.
    /// Checks for each table and runs the full migration if any are missing.
    async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
        // Check if the required tables exist
        let accounts_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='accounts')",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(false);

        let channels_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='channels')",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(false);

        let akick_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='channel_akick')"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(false);

        let history_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='message_history')"
        )
        .fetch_one(pool)
        .await
        .unwrap_or(false);

        // Run migrations if any tables are missing
        if !accounts_exists || !channels_exists || !akick_exists {
            // Run the full migration
            Self::run_migration_file(pool, include_str!("../../migrations/001_init.sql")).await;
            info!("Database migrations applied (001_init)");
        }

        // Always check for history table (newer migration)
        if !history_exists {
            Self::run_migration_file(pool, include_str!("../../migrations/003_history.sql")).await;
            info!("Database migrations applied (003_history)");
        }

        if accounts_exists && channels_exists && akick_exists && history_exists {
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

    /// Get history repository.
    pub fn history(&self) -> HistoryRepository<'_> {
        HistoryRepository::new(&self.pool)
    }
}
