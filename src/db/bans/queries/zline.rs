//! Z-line (IP ban, skips DNS) operations.

use super::super::models::Zline;
use super::generic::{add_ban, get_active_bans, matches_ban, remove_ban};
use crate::db::DbError;
use sqlx::SqlitePool;

/// Add a Z-line.
pub async fn add_zline(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    add_ban::<Zline>(pool, mask, reason, set_by, duration).await
}

/// Remove a Z-line.
pub async fn remove_zline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    remove_ban::<Zline>(pool, mask).await
}

/// Get all active Z-lines (not expired).
pub async fn get_active_zlines(pool: &SqlitePool) -> Result<Vec<Zline>, DbError> {
    get_active_bans::<Zline>(pool).await
}

/// Check if an IP matches any active Z-line.
pub async fn matches_zline(pool: &SqlitePool, ip: &str) -> Result<Option<Zline>, DbError> {
    matches_ban::<Zline>(pool, ip).await
}
