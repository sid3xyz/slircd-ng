//! Generic ban query operations.
//!
//! This module provides a trait-based generic implementation for all ban types
//! (K-line, D-line, G-line, Z-line, R-line, Shun), eliminating ~300 lines of
//! duplicated code across individual query files.

use crate::db::DbError;
use sqlx::SqlitePool;

/// Generic trait for ban types that can be queried from the database.
///
/// Implementors must provide:
/// - Table name for SQL queries
/// - Constructor from database row tuple
/// - Match function for checking if a target matches the ban mask
pub trait BanType: Sized + Clone {
    /// The database table name (e.g., "klines", "dlines").
    fn table_name() -> &'static str;

    /// Construct from database row: (mask, reason, set_by, set_at, expires_at).
    fn from_row(
        mask: String,
        reason: Option<String>,
        set_by: String,
        set_at: i64,
        expires_at: Option<i64>,
    ) -> Self;

    /// Check if the given target matches this ban's mask.
    fn matches(&self, target: &str) -> bool;
}

/// Generic implementation for adding a ban.
pub async fn add_ban<T: BanType>(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    let now = chrono::Utc::now().timestamp();
    let expires_at = duration.map(|d| now + d);

    let query = format!(
        r#"
        INSERT OR REPLACE INTO {} (mask, reason, set_by, set_at, expires_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
        T::table_name()
    );

    sqlx::query(&query)
        .bind(mask)
        .bind(reason)
        .bind(set_by)
        .bind(now)
        .bind(expires_at)
        .execute(pool)
        .await?;

    Ok(())
}

/// Generic implementation for removing a ban.
pub async fn remove_ban<T: BanType>(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    let query = format!("DELETE FROM {} WHERE mask = ?", T::table_name());

    let result = sqlx::query(&query).bind(mask).execute(pool).await?;

    Ok(result.rows_affected() > 0)
}

/// Generic implementation for getting all active bans (not expired).
pub async fn get_active_bans<T: BanType>(pool: &SqlitePool) -> Result<Vec<T>, DbError> {
    let now = chrono::Utc::now().timestamp();

    let query = format!(
        r#"
        SELECT mask, reason, set_by, set_at, expires_at
        FROM {}
        WHERE expires_at IS NULL OR expires_at > ?
        "#,
        T::table_name()
    );

    let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(&query)
        .bind(now)
        .fetch_all(pool)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(mask, reason, set_by, set_at, expires_at)| {
            T::from_row(mask, reason, set_by, set_at, expires_at)
        })
        .collect())
}

/// Generic implementation for checking if a target matches any active ban.
pub async fn matches_ban<T: BanType>(
    pool: &SqlitePool,
    target: &str,
) -> Result<Option<T>, DbError> {
    let bans = get_active_bans::<T>(pool).await?;

    for ban in bans {
        if ban.matches(target) {
            return Ok(Some(ban));
        }
    }

    Ok(None)
}
