//! R-line (realname ban) operations.

use super::super::models::Rline;
use super::generic::{add_ban, get_active_bans, matches_ban, remove_ban};
use crate::db::DbError;
use sqlx::SqlitePool;

/// Add an R-line.
pub async fn add_rline(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    add_ban::<Rline>(pool, mask, reason, set_by, duration).await
}

/// Remove an R-line.
pub async fn remove_rline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    remove_ban::<Rline>(pool, mask).await
}

/// Get all active R-lines (not expired).
#[allow(dead_code)] // Used by admin STATS command
pub async fn get_active_rlines(pool: &SqlitePool) -> Result<Vec<Rline>, DbError> {
    get_active_bans::<Rline>(pool).await
}

/// Check if a realname matches any active R-line.
pub async fn matches_rline(pool: &SqlitePool, realname: &str) -> Result<Option<Rline>, DbError> {
    matches_ban::<Rline>(pool, realname).await
}
