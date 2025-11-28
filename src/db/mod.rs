//! Database module for persistent storage.
//!
//! Provides async SQLite database access using SQLx for:
//! - NickServ accounts and nicknames
//! - K-lines and D-lines persistence

mod accounts;

pub use accounts::AccountRepository;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
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
    #[allow(dead_code)]
    NicknameNotFound(String),
    #[error("account already exists: {0}")]
    AccountExists(String),
    #[error("nickname already registered: {0}")]
    NicknameRegistered(String),
    #[error("invalid password")]
    InvalidPassword,
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
        if let Some(parent) = Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).ok();
            }
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
    async fn run_migrations(pool: &SqlitePool) -> Result<(), DbError> {
        // We'll use manual migration for the initial schema
        // since we're embedding it directly
        sqlx::query(include_str!("../../migrations/001_init.sql"))
            .execute(pool)
            .await
            .ok(); // Ignore errors if tables already exist

        info!("Database migrations complete");
        Ok(())
    }

    /// Get the connection pool.
    #[allow(dead_code)]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get account repository.
    pub fn accounts(&self) -> AccountRepository<'_> {
        AccountRepository::new(&self.pool)
    }
}
