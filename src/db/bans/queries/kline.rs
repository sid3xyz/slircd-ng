//! K-line (local user@host ban) operations.

use super::super::models::Kline;
use crate::db::DbError;
use slirc_proto::wildcard_match;
use sqlx::SqlitePool;

/// Add a K-line.
pub async fn add_kline(
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
        INSERT OR REPLACE INTO klines (mask, reason, set_by, set_at, expires_at)
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

/// Remove a K-line.
pub async fn remove_kline(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    let result = sqlx::query("DELETE FROM klines WHERE mask = ?")
        .bind(mask)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Get all active K-lines (not expired).
pub async fn get_active_klines(pool: &SqlitePool) -> Result<Vec<Kline>, DbError> {
    let now = chrono::Utc::now().timestamp();

    let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
        r#"
        SELECT mask, reason, set_by, set_at, expires_at
        FROM klines
        WHERE expires_at IS NULL OR expires_at > ?
        "#,
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(mask, reason, set_by, set_at, expires_at)| Kline {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        })
        .collect())
}

/// Check if a user@host matches any active K-line.
pub async fn matches_kline(pool: &SqlitePool, user_host: &str) -> Result<Option<Kline>, DbError> {
    let klines = get_active_klines(pool).await?;

    for kline in klines {
        if wildcard_match(&kline.mask, user_host) {
            return Ok(Some(kline));
        }
    }

    Ok(None)
}
