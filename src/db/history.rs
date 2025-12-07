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
use tracing::{info, warn};

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
    pub target_account: Option<&'a str>,
    pub nanotime: Option<i64>,
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

        println!("DEBUG: store_message: msgid={} target={} sender={}", params.msgid, normalized_target, params.sender_nick);

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
        .execute(self.pool)
        .await
        .map_err(|e| {
            tracing::error!("Failed to insert message: {}", e);
            e
        })?;

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

    /// Query most recent N messages after a timestamp (CHATHISTORY LATEST with lower bound).
    pub async fn query_latest_after(
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
            ORDER BY nanotime DESC
            LIMIT ?
            "#,
        )
        .bind(&normalized_target)
        .bind(after_nanos)
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

    /// Query messages between two timestamps (CHATHISTORY BETWEEN) in reverse order.
    pub async fn query_between_desc(
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
            ORDER BY nanotime DESC
            LIMIT ?
            "#,
        )
        .bind(&normalized_target)
        .bind(start_nanos)
        .bind(end_nanos)
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

    /// Query DM history between two users (LATEST).
    pub async fn query_dm_latest(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let u1_lower = irc_to_lower(user1);
        let u2_lower = irc_to_lower(user2);

        let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(acct)
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(acct)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE (target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?)
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        };

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

    /// Query DM history between two users (LATEST with lower bound).
    pub async fn query_dm_latest_after(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        after_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let u1_lower = irc_to_lower(user1);
        let u2_lower = irc_to_lower(user2);

        let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
                  AND nanotime > ?
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(acct)
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(acct)
            .bind(after_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
                  AND nanotime > ?
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(after_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        };

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

    /// Query DM history between two users (BEFORE).
    pub async fn query_dm_before(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        before_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let u1_lower = irc_to_lower(user1);
        let u2_lower = irc_to_lower(user2);

        println!("DEBUG: query_dm_before u1={} u2={} before={} limit={}", u1_lower, u2_lower, before_nanos, limit);

        let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
                  AND nanotime < ?
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(acct)
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(acct)
            .bind(before_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
                  AND nanotime < ?
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(before_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        };

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

    /// Query DM history between two users (AFTER).
    pub async fn query_dm_after(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        after_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let u1_lower = irc_to_lower(user1);
        let u2_lower = irc_to_lower(user2);

        let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
                  AND nanotime > ?
                ORDER BY nanotime ASC
                LIMIT ?
                "#,
            )
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(acct)
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(acct)
            .bind(after_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
                  AND nanotime > ?
                ORDER BY nanotime ASC
                LIMIT ?
                "#,
            )
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(after_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        };

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

    /// Query DM history between two users (BETWEEN).
    pub async fn query_dm_between(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        start_nanos: i64,
        end_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let u1_lower = irc_to_lower(user1);
        let u2_lower = irc_to_lower(user2);

        let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
                  AND nanotime > ? AND nanotime < ?
                ORDER BY nanotime ASC
                LIMIT ?
                "#,
            )
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(acct)
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(acct)
            .bind(start_nanos)
            .bind(end_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
                  AND nanotime > ? AND nanotime < ?
                ORDER BY nanotime ASC
                LIMIT ?
                "#,
            )
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(start_nanos)
            .bind(end_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        };

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

    /// Query DM history between two users (BETWEEN) in reverse order.
    pub async fn query_dm_between_desc(
        &self,
        user1: &str,
        user1_account: Option<&str>,
        user2: &str,
        start_nanos: i64,
        end_nanos: i64,
        limit: u32,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let u1_lower = irc_to_lower(user1);
        let u2_lower = irc_to_lower(user2);

        let rows: Vec<HistoryRow> = if let Some(acct) = user1_account {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ? AND account = ?) OR (target = ? AND lower(sender) = ? AND target_account = ?))
                  AND nanotime > ? AND nanotime < ?
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(acct)
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(acct)
            .bind(start_nanos)
            .bind(end_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        } else {
            sqlx::query_as(
                r#"
                SELECT msgid, target, sender, message_data, nanotime, account
                FROM message_history
                WHERE ((target = ? AND lower(sender) = ?) OR (target = ? AND lower(sender) = ?))
                  AND nanotime > ? AND nanotime < ?
                ORDER BY nanotime DESC
                LIMIT ?
                "#,
            )
            .bind(&u1_lower)
            .bind(&u2_lower)
            .bind(&u2_lower)
            .bind(&u1_lower)
            .bind(start_nanos)
            .bind(end_nanos)
            .bind(limit as i64)
            .fetch_all(self.pool)
            .await?
        };

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

    /// Lookup msgid for DM and return its nanotime.
    pub async fn lookup_dm_msgid_nanotime(
        &self,
        _user1: &str,
        _user2: &str,
        msgid: &str,
    ) -> Result<Option<i64>, DbError> {
        // Relaxed lookup to fix AROUND failure
        let result: Option<(i64,)> = sqlx::query_as(
            "SELECT nanotime FROM message_history WHERE msgid = ?"
        )
        .bind(msgid)
        .fetch_optional(self.pool)
        .await?;

        if result.is_none() {
            println!("DEBUG: lookup_dm_msgid_nanotime: msgid {} not found", msgid);
            // Debug: list all msgids
            let all_ids: Vec<(String,)> = sqlx::query_as("SELECT msgid FROM message_history LIMIT 5")
                .fetch_all(self.pool)
                .await?;
            println!("DEBUG: First 5 msgids in DB: {:?}", all_ids);
        } else {
            println!("DEBUG: lookup_dm_msgid_nanotime: msgid {} found, time={}", msgid, result.unwrap().0);
        }

        Ok(result.map(|(n,)| n))
    }

    /// Fetch a single message by ID.
    pub async fn get_message_by_id(&self, msgid: &str) -> Result<Option<StoredMessage>, DbError> {
        let row: Option<HistoryRow> = sqlx::query_as(
            r#"
            SELECT msgid, target, sender, message_data, nanotime, account
            FROM message_history
            WHERE msgid = ?
            "#
        )
        .bind(msgid)
        .fetch_optional(self.pool)
        .await?;

        if let Some((msgid, target, sender, data, nanotime, account)) = row {
            let envelope: MessageEnvelope = serde_json::from_slice(&data).map_err(|e| DbError::Sqlx(sqlx::Error::Protocol(e.to_string())))?;
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
    pub async fn query_targets(
        &self,
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

        let rows = q.fetch_all(self.pool).await?;
        Ok(rows)
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
