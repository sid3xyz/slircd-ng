//! G-line (global user@host ban) operations.

use super::super::models::Gline;
use super::generic::{add_ban, get_active_bans, matches_ban, remove_ban};
use crate::db::DbError;
use sqlx::SqlitePool;

/// Add a G-line.
pub async fn add_gline(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    add_ban::<Gline>(pool, mask, reason, set_by, duration).await
}

/// Remove a G-line.
pub async fn remove_gline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    remove_ban::<Gline>(pool, mask).await
}

/// Get all active G-lines (not expired).
pub async fn get_active_glines(pool: &SqlitePool) -> Result<Vec<Gline>, DbError> {
    get_active_bans::<Gline>(pool).await
}

/// Check if a user@host matches any active G-line.
pub async fn matches_gline(pool: &SqlitePool, user_host: &str) -> Result<Option<Gline>, DbError> {
    matches_ban::<Gline>(pool, user_host).await
}
