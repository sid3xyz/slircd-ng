//! R-line (realname ban) operations.

use super::super::models::Rline;
use crate::db::DbError;
use slirc_proto::wildcard_match;
use sqlx::SqlitePool;

/// Add an R-line.
pub async fn add_rline(
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
        INSERT OR REPLACE INTO rlines (mask, reason, set_by, set_at, expires_at)
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

/// Remove an R-line.
pub async fn remove_rline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    let result = sqlx::query("DELETE FROM rlines WHERE mask = ?")
        .bind(mask)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Get all active R-lines (not expired).
pub async fn get_active_rlines(pool: &SqlitePool) -> Result<Vec<Rline>, DbError> {
    let now = chrono::Utc::now().timestamp();

    let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
        r#"
        SELECT mask, reason, set_by, set_at, expires_at
        FROM rlines
        WHERE expires_at IS NULL OR expires_at > ?
        "#,
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(mask, reason, set_by, set_at, expires_at)| Rline {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        })
        .collect())
}

/// Check if a realname matches any active R-line.
pub async fn matches_rline(pool: &SqlitePool, realname: &str) -> Result<Option<Rline>, DbError> {
    let rlines = get_active_rlines(pool).await?;

    for rline in rlines {
        if wildcard_match(&rline.mask, realname) {
            return Ok(Some(rline));
        }
    }

    Ok(None)
}
