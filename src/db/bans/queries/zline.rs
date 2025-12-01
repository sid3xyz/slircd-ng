//! Z-line (IP ban, skips DNS) operations.

use super::super::models::{Zline, cidr_match};
use crate::db::DbError;
use slirc_proto::wildcard_match;
use sqlx::SqlitePool;

/// Add a Z-line.
#[allow(dead_code)] // Phase 3b: Admin commands
pub async fn add_zline(
    pool: &SqlitePool,
    mask: &str,
    reason: Option<&str>,
    set_by: &str,
    duration: Option<i64>,
) -> Result<(), DbError> {
    let now = chrono::Utc::now().timestamp();
    let expires_at = duration.map(|d| now + d);

    sqlx::query(
        r#"
        INSERT OR REPLACE INTO zlines (mask, reason, set_by, set_at, expires_at)
        VALUES (?, ?, ?, ?, ?)
        "#,
    )
    .bind(mask)
    .bind(reason)
    .bind(set_by)
    .bind(now)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(())
}

/// Remove a Z-line.
#[allow(dead_code)] // Phase 3b: Admin commands
pub async fn remove_zline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    let result = sqlx::query("DELETE FROM zlines WHERE mask = ?")
        .bind(mask)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Get all active Z-lines (not expired).
pub async fn get_active_zlines(pool: &SqlitePool) -> Result<Vec<Zline>, DbError> {
    let now = chrono::Utc::now().timestamp();

    let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
        r#"
        SELECT mask, reason, set_by, set_at, expires_at
        FROM zlines
        WHERE expires_at IS NULL OR expires_at > ?
        "#,
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(mask, reason, set_by, set_at, expires_at)| Zline {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        })
        .collect())
}

/// Check if an IP matches any active Z-line.
pub async fn matches_zline(pool: &SqlitePool, ip: &str) -> Result<Option<Zline>, DbError> {
    let zlines = get_active_zlines(pool).await?;

    for zline in zlines {
        if wildcard_match(&zline.mask, ip) || cidr_match(&zline.mask, ip) {
            return Ok(Some(zline));
        }
    }

    Ok(None)
}
