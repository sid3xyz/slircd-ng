//! Core storage operations for message history.

use crate::db::DbError;
use super::types::{HistoryRow, MessageEnvelope, StoreMessageParams, StoredMessage};
use slirc_proto::irc_to_lower;
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

/// Store a channel message in history.
///
/// Uses nanosecond timestamps for precise ordering.
/// Idempotent: duplicate msgids are ignored.
pub(super) async fn store_message(
    pool: &SqlitePool,
    params: StoreMessageParams<'_>,
) -> Result<(), DbError> {
    let normalized_target = irc_to_lower(params.channel);

    let nanotime = params.nanotime.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0)
    });

    let envelope = MessageEnvelope {
        command: "PRIVMSG".to_string(),
        prefix: params.prefix.to_string(),
        target: params.channel.to_string(),
        text: params.text.to_string(),
        tags: None,
    };

    let message_data = serde_json::to_vec(&envelope)
        .map_err(|e| DbError::Sqlx(sqlx::Error::Protocol(e.to_string())))?;

    sqlx::query(
        r#"
        INSERT OR IGNORE INTO message_history (msgid, target, sender, message_data, nanotime, account, target_account)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(params.msgid)
    .bind(&normalized_target)
    .bind(params.sender_nick)
    .bind(&message_data)
    .bind(nanotime)
    .bind(params.account)
    .bind(params.target_account)
    .execute(pool)
    .await
    .map_err(|e| {
        tracing::error!("Failed to insert message: {}", e);
        e
    })?;

    Ok(())
}

/// Fetch a single message by ID.
pub(super) async fn get_message_by_id(
    pool: &SqlitePool,
    msgid: &str,
) -> Result<Option<StoredMessage>, DbError> {
    let row: Option<HistoryRow> = sqlx::query_as(
        r#"
        SELECT msgid, target, sender, message_data, nanotime, account
        FROM message_history
        WHERE msgid = ?
        "#
    )
    .bind(msgid)
    .fetch_optional(pool)
    .await?;

    if let Some((msgid, target, sender, data, nanotime, account)) = row {
        let envelope: MessageEnvelope = serde_json::from_slice(&data)
            .map_err(|e| DbError::Sqlx(sqlx::Error::Protocol(e.to_string())))?;
        Ok(Some(StoredMessage {
            msgid,
            target,
            sender,
            envelope,
            nanotime,
            account,
        }))
    } else {
        Ok(None)
    }
}

/// Query targets (channels and DMs) with activity between start and end.
/// Returns list of (target_name, last_timestamp).
pub(super) async fn query_targets(
    pool: &SqlitePool,
    nick: &str,
    channels: &[String],
    start: i64,
    end: i64,
    limit: usize,
) -> Result<Vec<(String, i64)>, DbError> {
    let nick_lower = irc_to_lower(nick);

    let mut query = String::from(
        r#"
        SELECT other, MAX(nanotime) as last_time FROM (
            SELECT lower(sender) as other, nanotime
            FROM message_history
            WHERE target = ? AND target NOT LIKE '#%'

            UNION ALL

            SELECT target as other, nanotime
            FROM message_history
            WHERE lower(sender) = ? AND target NOT LIKE '#%'
        "#
    );

    if !channels.is_empty() {
        query.push_str(" UNION ALL SELECT target as other, nanotime FROM message_history WHERE target IN (");
        for (i, _) in channels.iter().enumerate() {
            if i > 0 {
                query.push_str(", ");
            }
            query.push('?');
        }
        query.push_str(") ");
    }

    query.push_str(
        r#"
        )
        GROUP BY other
        HAVING last_time > ? AND last_time < ?
        ORDER BY last_time ASC
        LIMIT ?
        "#
    );

    let mut q = sqlx::query_as::<_, (String, i64)>(&query);

    q = q.bind(&nick_lower);
    q = q.bind(&nick_lower);

    for chan in channels {
        q = q.bind(irc_to_lower(chan));
    }

    q = q.bind(start);
    q = q.bind(end);
    q = q.bind(limit as i64);

    let rows = q.fetch_all(pool).await?;
    Ok(rows)
}

/// Prune old messages based on retention policy.
///
/// Called by scheduled maintenance task in main.rs (runs daily).
pub(super) async fn prune_old_messages(
    pool: &SqlitePool,
    retention_days: u32,
) -> Result<u64, DbError> {
    let retention_nanos = (retention_days as i64) * 86400 * 1_000_000_000;
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0);
    let cutoff = now_nanos - retention_nanos;

    let result = sqlx::query("DELETE FROM message_history WHERE nanotime < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;

    Ok(result.rows_affected())
}
