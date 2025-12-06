//! D-line (IP ban) operations.

use super::super::models::Dline;
use super::generic::{add_ban, get_active_bans, matches_ban, remove_ban};
use crate::db::DbError;
use sqlx::SqlitePool;

/// Add a D-line.
pub async fn add_dline(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    add_ban::<Dline>(pool, mask, reason, set_by, duration).await
}

/// Remove a D-line.
pub async fn remove_dline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    remove_ban::<Dline>(pool, mask).await
}

/// Get all active D-lines (not expired).
pub async fn get_active_dlines(pool: &SqlitePool) -> Result<Vec<Dline>, DbError> {
    get_active_bans::<Dline>(pool).await
}

/// Check if an IP matches any active D-line.
pub async fn matches_dline(pool: &SqlitePool, ip: &str) -> Result<Option<Dline>, DbError> {
    matches_ban::<Dline>(pool, ip).await
}
