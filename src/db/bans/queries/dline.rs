//! D-line (IP ban) operations.

use super::super::models::{Dline, cidr_match};
use crate::db::DbError;
use slirc_proto::wildcard_match;
use sqlx::SqlitePool;

/// Add a D-line.
pub async fn add_dline(
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
        INSERT OR REPLACE INTO dlines (mask, reason, set_by, set_at, expires_at)
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

/// Remove a D-line.
pub async fn remove_dline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    let result = sqlx::query("DELETE FROM dlines WHERE mask = ?")
        .bind(mask)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Get all active D-lines (not expired).
pub async fn get_active_dlines(pool: &SqlitePool) -> Result<Vec<Dline>, DbError> {
    let now = chrono::Utc::now().timestamp();

    let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
        r#"
        SELECT mask, reason, set_by, set_at, expires_at
        FROM dlines
        WHERE expires_at IS NULL OR expires_at > ?
        "#,
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(mask, reason, set_by, set_at, expires_at)| Dline {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        })
        .collect())
}

/// Check if an IP matches any active D-line.
pub async fn matches_dline(pool: &SqlitePool, ip: &str) -> Result<Option<Dline>, DbError> {
    let dlines = get_active_dlines(pool).await?;

    for dline in dlines {
        if wildcard_match(&dline.mask, ip) || cidr_match(&dline.mask, ip) {
            return Ok(Some(dline));
        }
    }

    Ok(None)
}
