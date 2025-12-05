//! Message history storage for CHATHISTORY command (IRCv3 draft/chathistory).
//!
//! Provides persistent message storage for channel history retrieval.
//!
//! # Reference
//! - IRCv3 chathistory: <https://ircv3.net/specs/extensions/chathistory>
//!
//! # Architecture
//! - Uses sqlx async SQLite (consistent with rest of db layer)
//! - JSON message envelope for flexible schema evolution
//! - Nanosecond timestamps for precise ordering
//! - Storage is async and non-blocking

use crate::db::DbError;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use slirc_proto::irc_to_lower;
use sqlx::SqlitePool;
use std::time::{SystemTime, UNIX_EPOCH};

/// Row type from database query: (msgid, target, sender, message_data, nanotime, account)
type HistoryRow = (String, String, String, Vec<u8>, i64, Option<String>);

/// Message envelope stored as JSON BLOB.
/// Allows adding fields without schema migrations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    /// Command type ("PRIVMSG" or "NOTICE")
    pub command: String,
    /// Full sender prefix (nick!user@host)
    pub prefix: String,
    /// Target channel or nickname
    pub target: String,
    /// Message text content
    pub text: String,
    /// IRCv3 message tags (preserved for replay)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<MessageTag>>,
}

/// IRCv3 message tag for history storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTag {
    pub key: String,
    pub value: Option<String>,
}

/// Parameters for storing a channel message.
pub struct StoreMessageParams<'a> {
    pub msgid: &'a str,
    pub channel: &'a str,
    pub sender_nick: &'a str,
    pub prefix: &'a str,
    pub text: &'a str,
    pub account: Option<&'a str>,
}

/// Stored message retrieved from database.
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub msgid: String,
    /// Target channel (lowercased for lookup).
    /// Used for debugging/logging; envelope.target contains display name.
    #[allow(dead_code)] // Retained for debugging and future TARGETS subcommand
    pub target: String,
    /// Sender nickname (for filtering).
    /// Used for debugging/logging and future sender-based filtering.
    #[allow(dead_code)] // Retained for debugging and future sender filtering
    pub sender: String,
    pub envelope: MessageEnvelope,
    pub nanotime: i64,
    pub account: Option<String>,
}

impl StoredMessage {
    /// Convert nanotime to ISO8601 timestamp for IRCv3 server-time tag.
    pub fn timestamp_iso(&self) -> String {
        let secs = self.nanotime / 1_000_000_000;
        let nanos = (self.nanotime % 1_000_000_000) as u32;

        if let Some(dt) = DateTime::<Utc>::from_timestamp(secs, nanos) {
            dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        } else {
            "1970-01-01T00:00:00.000Z".to_string()
        }
    }
}

/// History repository for message storage and retrieval.
pub struct HistoryRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> HistoryRepository<'a> {
    /// Create a new history repository.
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Store a channel message in history.
    ///
    /// Uses nanosecond timestamps for precise ordering.
    /// Idempotent: duplicate msgids are ignored.
    pub async fn store_message(&self, params: StoreMessageParams<'_>) -> Result<(), DbError> {
        let normalized_target = irc_to_lower(params.channel);

        let nanotime = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);

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
            INSERT OR IGNORE INTO message_history (msgid, target, sender, message_data, nanotime, account)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(params.msgid)
        .bind(&normalized_target)
        .bind(params.sender_nick)
        .bind(&message_data)
        .bind(nanotime)
        .bind(params.account)
        .execute(self.pool)
        .await?;

        Ok(())
    }

    /// Query most recent N messages (CHATHISTORY LATEST).
    pub async fn query_latest(
        &self,
        target: &str,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let normalized_target = irc_to_lower(target);

        let rows: Vec<HistoryRow> = sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE target = ?
            ORDER BY nanotime DESC
            LIMIT ?
            "#,
        )
        .bind(&normalized_target)
        .bind(limit as i64)
        .fetch_all(self.pool)
        .await?;

        let mut messages: Vec<StoredMessage> = rows
            .into_iter()
            .filter_map(|(msgid, target, sender, data, nanotime, account)| {
                let envelope: MessageEnvelope = serde_json::from_slice(&data).ok()?;
                Some(StoredMessage {
                    msgid,
                    target,
                    sender,
                    envelope,
                    nanotime,
                    account,
                })
            })
            .collect();

        // Reverse to chronological order (oldest first)
        messages.reverse();
        Ok(messages)
    }

    /// Query messages before a timestamp (CHATHISTORY BEFORE).
    pub async fn query_before(
        &self,
        target: &str,
        before_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let normalized_target = irc_to_lower(target);

        let rows: Vec<HistoryRow> = sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE target = ? AND nanotime < ?
            ORDER BY nanotime DESC
            LIMIT ?
            "#,
        )
        .bind(&normalized_target)
        .bind(before_nanos)
        .bind(limit as i64)
        .fetch_all(self.pool)
        .await?;

        let mut messages: Vec<StoredMessage> = rows
            .into_iter()
            .filter_map(|(msgid, target, sender, data, nanotime, account)| {
                let envelope: MessageEnvelope = serde_json::from_slice(&data).ok()?;
                Some(StoredMessage {
                    msgid,
                    target,
                    sender,
                    envelope,
                    nanotime,
                    account,
                })
            })
            .collect();

        messages.reverse();
        Ok(messages)
    }

    /// Query messages after a timestamp (CHATHISTORY AFTER).
    pub async fn query_after(
        &self,
        target: &str,
        after_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let normalized_target = irc_to_lower(target);

        let rows: Vec<HistoryRow> = sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE target = ? AND nanotime > ?
            ORDER BY nanotime ASC
            LIMIT ?
            "#,
        )
        .bind(&normalized_target)
        .bind(after_nanos)
        .bind(limit as i64)
        .fetch_all(self.pool)
        .await?;

        let messages: Vec<StoredMessage> = rows
            .into_iter()
            .filter_map(|(msgid, target, sender, data, nanotime, account)| {
                let envelope: MessageEnvelope = serde_json::from_slice(&data).ok()?;
                Some(StoredMessage {
                    msgid,
                    target,
                    sender,
                    envelope,
                    nanotime,
                    account,
                })
            })
            .collect();

        Ok(messages)
    }

    /// Query messages between two timestamps (CHATHISTORY BETWEEN).
    pub async fn query_between(
        &self,
        target: &str,
        start_nanos: i64,
        end_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let normalized_target = irc_to_lower(target);

        let rows: Vec<HistoryRow> = sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE target = ? AND nanotime > ? AND nanotime < ?
            ORDER BY nanotime ASC
            LIMIT ?
            "#,
        )
        .bind(&normalized_target)
        .bind(start_nanos)
        .bind(end_nanos)
        .bind(limit as i64)
        .fetch_all(self.pool)
        .await?;

        let messages: Vec<StoredMessage> = rows
            .into_iter()
            .filter_map(|(msgid, target, sender, data, nanotime, account)| {
                let envelope: MessageEnvelope = serde_json::from_slice(&data).ok()?;
                Some(StoredMessage {
                    msgid,
                    target,
                    sender,
                    envelope,
                    nanotime,
                    account,
                })
            })
            .collect();

        Ok(messages)
    }

    /// Query messages around a timestamp (CHATHISTORY AROUND).
    pub async fn query_around(
        &self,
        target: &str,
        around_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let half = limit / 2;

        let mut before = self.query_before(target, around_nanos, half).await?;
        let after = self.query_after(target, around_nanos, half).await?;

        before.extend(after);
        Ok(before)
    }

    /// Lookup msgid and return its nanotime.
    pub async fn lookup_msgid_nanotime(
        &self,
        target: &str,
        msgid: &str,
    ) -> Result<Option<i64>, DbError> {
        let normalized_target = irc_to_lower(target);

        let result: Option<(i64,)> =
            sqlx::query_as("SELECT nanotime FROM message_history WHERE target = ? AND msgid = ?")
                .bind(&normalized_target)
                .bind(msgid)
                .fetch_optional(self.pool)
                .await?;

        Ok(result.map(|(n,)| n))
    }

    /// Prune old messages based on retention policy.
    ///
    /// Called by scheduled maintenance task in main.rs (runs daily).
    pub async fn prune_old_messages(&self, retention_days: u32) -> Result<u64, DbError> {
        let retention_nanos = (retention_days as i64) * 86400 * 1_000_000_000;
        let now_nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);
        let cutoff = now_nanos - retention_nanos;

        let result = sqlx::query("DELETE FROM message_history WHERE nanotime < ?")
            .bind(cutoff)
            .execute(self.pool)
            .await?;

        Ok(result.rows_affected())
    }
}
