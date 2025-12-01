//! Shun (silent ignore) operations.

use super::super::models::{Shun, cidr_match};
use crate::db::DbError;
use slirc_proto::wildcard_match;
use sqlx::SqlitePool;

/// Add a shun.
pub async fn add_shun(
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
        INSERT OR REPLACE INTO shuns (mask, reason, set_by, set_at, expires_at)
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

/// Remove a shun.
pub async fn remove_shun(pool: &SqlitePool, mask: &str) -> Result<bool, DbError> {
    let result = sqlx::query("DELETE FROM shuns WHERE mask = ?")
        .bind(mask)
        .execute(pool)
        .await?;

    Ok(result.rows_affected() > 0)
}

/// Get all active shuns (not expired).
pub async fn get_active_shuns(pool: &SqlitePool) -> Result<Vec<Shun>, DbError> {
    let now = chrono::Utc::now().timestamp();

    let rows = sqlx::query_as::<_, (String, Option<String>, String, i64, Option<i64>)>(
        r#"
        SELECT mask, reason, set_by, set_at, expires_at
        FROM shuns
        WHERE expires_at IS NULL OR expires_at > ?
        "#,
    )
    .bind(now)
    .fetch_all(pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(mask, reason, set_by, set_at, expires_at)| Shun {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        })
        .collect())
}

/// Check if a user@host matches any active shun.
pub async fn matches_shun(pool: &SqlitePool, user_host: &str) -> Result<Option<Shun>, DbError> {
    let shuns = get_active_shuns(pool).await?;

    for shun in shuns {
        if wildcard_match(&shun.mask, user_host) {
            return Ok(Some(shun));
        }
    }

    Ok(None)
}

/// Check if an IP matches any active shun.
#[allow(dead_code)] // Will be used for connection-time shun checks
pub async fn matches_shun_ip(pool: &SqlitePool, ip: &str) -> Result<Option<Shun>, DbError> {
    let shuns = get_active_shuns(pool).await?;

    for shun in shuns {
        if wildcard_match(&shun.mask, ip) || cidr_match(&shun.mask, ip) {
            return Ok(Some(shun));
        }
    }

    Ok(None)
}
