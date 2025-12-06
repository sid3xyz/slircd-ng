//! K-line (local user@host ban) operations.

use super::super::models::Kline;
use super::generic::{add_ban, get_active_bans, matches_ban, remove_ban};
use crate::db::DbError;
use sqlx::SqlitePool;

/// Add a K-line.
pub async fn add_kline(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    add_ban::<Kline>(pool, mask, reason, set_by, duration).await
}

/// Remove a K-line.
pub async fn remove_kline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    remove_ban::<Kline>(pool, mask).await
}

/// Get all active K-lines (not expired).
pub async fn get_active_klines(pool: &SqlitePool) -> Result<Vec<Kline>, DbError> {
    get_active_bans::<Kline>(pool).await
}

/// Check if a user@host matches any active K-line.
pub async fn matches_kline(pool: &SqlitePool, user_host: &str) -> Result<Option<Kline>, DbError> {
    matches_ban::<Kline>(pool, user_host).await
}
