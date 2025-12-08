//! Shun (silent ignore) operations.

use super::super::models::Shun;
use super::generic::{add_ban, get_active_bans, matches_ban, remove_ban};
use crate::db::DbError;
use sqlx::SqlitePool;

/// Add a shun.
pub async fn add_shun(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    add_ban::<Shun>(pool, mask, reason, set_by, duration).await
}

/// Remove a shun.
pub async fn remove_shun(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    remove_ban::<Shun>(pool, mask).await
}

/// Get all active shuns (not expired).
pub async fn get_active_shuns(pool: &SqlitePool) -> Result<Vec<Shun>, DbError> {
    get_active_bans::<Shun>(pool).await
}

/// Check if a user@host matches any active shun.
pub async fn matches_shun(pool: &SqlitePool, user_host: &str) -> Result<Option<Shun>, DbError> {
    matches_ban::<Shun>(pool, user_host).await
}
