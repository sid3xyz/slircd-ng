//! Message history storage for CHATHISTORY command (IRCv3 draft/chathistory)
//!
//! RFC: https://ircv3.net/specs/extensions/chathistory
//!
//! IMPLEMENTATION:
//!   - SQLite async via deadpool integration (non-blocking I/O path)
//!   - Privacy-first PM storage (disabled by default, configurable)
//!   - JSON message envelope (flexible schema evolution)
//!   - Nanosecond timestamps for precise ordering
//!   - Async batch insertion for performance

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::warn;

use crate::infrastructure::persistence::database::Database;
use crate::core::state::normalize_channel;

/// Message envelope for BLOB storage (flexible schema evolution)
/// Stored as JSON BLOB to allow adding fields without schema migrations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEnvelope {
    pub command: String,        // "PRIVMSG" or "NOTICE"
    pub prefix: String,         // Full sender prefix (nick!user@host)
    pub target: String,         // Channel or nickname
    pub text: String,           // Message content
    pub tags: Option<Vec<Tag>>, // IRCv3 tags (if any)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tag {
    pub key: String,
    pub value: Option<String>,
}

/// Parameters for storing a channel message (reduces clippy::too_many_arguments warnings)
pub struct ChannelMessageParams<'a> {
    pub msgid: &'a str,
    pub channel: &'a str,
    pub sender_nick: &'a str,
    pub prefix: &'a str,
    pub text: &'a str,
    pub account: Option<&'a str>,
    pub tags: Option<Vec<Tag>>,
}

/// Parameters for storing a private message (reduces clippy::too_many_arguments warnings)
pub struct PrivateMessageParams<'a> {
    pub msgid: &'a str,
    pub sender_nick: &'a str,
    pub recipient_nick: &'a str,
    pub prefix: &'a str,
    pub text: &'a str,
    pub account: Option<&'a str>,
    pub tags: Option<Vec<Tag>>,
}

/// Store a channel message in history with IRCv3 tags
/// Uses nanosecond timestamps for precise ordering of high-velocity channels
/// IRCv3 SPEC: Preserves all message tags (msgid, server-time, account-tag, etc.)
/// for accurate CHATHISTORY replay
pub async fn store_channel_message(db: &Database, params: ChannelMessageParams<'_>) -> Result<()> {
    // Normalize channel name for consistent indexing (case-insensitive per RFC1459)
    let normalized_target = normalize_channel(params.channel);

    // Get nanosecond timestamp for precise message ordering
    let nanotime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before UNIX epoch")?
        .as_nanos() as i64;

    // Build message envelope with IRCv3 tags (if provided)
    // PROTOCOL EXPERT: ✅ IRCv3 chathistory - preserve original message metadata
    // https://ircv3.net/specs/extensions/chathistory
    let envelope = MessageEnvelope {
        command: "PRIVMSG".to_string(),
        prefix: params.prefix.to_string(),
        target: params.channel.to_string(),
        text: params.text.to_string(),
        tags: params.tags, // Store all IRCv3 tags for accurate replay
    };

    let message_data = serde_json::to_vec(&envelope).context("serializing message envelope")?;

    let msgid = params.msgid.to_string();
    let sender_nick = params.sender_nick.to_string();
    let account = params.account.map(|s| s.to_string());

    // Async database insertion (non-blocking I/O path)
    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection for history storage")?;

    conn.interact(move |conn| {
        conn.execute(
            r"INSERT INTO message_history (msgid, target, sender, message_data, nanotime, account)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6)
              ON CONFLICT(msgid) DO NOTHING", // Idempotent insert
            rusqlite::params![
                msgid,
                normalized_target,
                sender_nick,
                message_data,
                nanotime,
                account
            ],
        )
        .context("inserting channel message into history")
    })
    .await
    .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(())
}

/// Store a private message in history (if enabled in config) with IRCv3 tags
/// PRIVACY: PM history is disabled by default (clear opt-in required)
/// Stored separately from channel history for granular retention control
/// IRCv3 SPEC: Preserves all message tags for accurate CHATHISTORY replay
pub async fn store_private_message(db: &Database, params: PrivateMessageParams<'_>) -> Result<()> {
    let nanotime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before UNIX epoch")?
        .as_nanos() as i64;

    // Build message envelope with IRCv3 tags (if provided)
    let envelope = MessageEnvelope {
        command: "PRIVMSG".to_string(),
        prefix: params.prefix.to_string(),
        target: params.recipient_nick.to_string(),
        text: params.text.to_string(),
        tags: params.tags, // Store all IRCv3 tags for accurate replay
    };

    let message_data = serde_json::to_vec(&envelope).context("serializing message envelope")?;

    let msgid = params.msgid.to_string();
    let sender_nick = params.sender_nick.to_string();
    let recipient_nick = params.recipient_nick.to_string();
    let account = params.account.map(|s| s.to_string());

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection for PM history storage")?;

    conn.interact(move |conn| {
        conn.execute(
            r"INSERT INTO private_message_history (msgid, sender, recipient, message_data, nanotime, account)
              VALUES (?1, ?2, ?3, ?4, ?5, ?6)
              ON CONFLICT(msgid) DO NOTHING",
            rusqlite::params![msgid, sender_nick, recipient_nick, message_data, nanotime, account],
        )
        .context("inserting private message into history")
    })
    .await
    .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(())
}

/// Background task to prune old messages based on retention policy
/// Automatic cleanup ensures database doesn't grow unbounded
/// Retention period configured per server policy (default: 30 days)
pub async fn prune_old_messages(db: &Database, retention_days: u32) -> Result<()> {
    let retention_seconds = (retention_days as i64) * 86400;
    let cutoff_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system time before UNIX epoch")?
        .as_secs() as i64
        - retention_seconds;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection for pruning")?;

    let deleted = conn
        .interact(move |conn| {
            let channel_deleted = conn
                .execute(
                    "DELETE FROM message_history WHERE created_at < ?1",
                    rusqlite::params![cutoff_time],
                )
                .context("pruning old channel messages")?;

            let pm_deleted = conn
                .execute(
                    "DELETE FROM private_message_history WHERE created_at < ?1",
                    rusqlite::params![cutoff_time],
                )
                .context("pruning old private messages")?;

            Ok::<_, anyhow::Error>((channel_deleted, pm_deleted))
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    if deleted.0 > 0 || deleted.1 > 0 {
        tracing::info!(
            channel_messages = deleted.0,
            private_messages = deleted.1,
            retention_days,
            "pruned old message history"
        );
    }

    Ok(())
}

/// Log history storage errors without failing message delivery
/// COMPETITIVE PATTERN: History storage should never block or fail message delivery
pub fn log_history_error(context: &str, err: anyhow::Error) {
    warn!(
        context = %context,
        error = ?err,
        "failed to store message history (delivery unaffected)"
    );
}

/// Stored message from database for CHATHISTORY responses
#[derive(Debug, Clone)]
pub struct StoredMessage {
    pub msgid: String,
    pub target: String,
    pub sender: String,
    pub message_data: String, // JSON envelope
    pub nanotime: i64,
    pub account: Option<String>,
    pub created_at: i64,
}

impl StoredMessage {
    /// Convert nanotime to ISO8601 timestamp for IRCv3 server-time tag
    pub fn timestamp_iso(&self) -> String {
        let secs = self.nanotime / 1_000_000_000;
        let nanos = (self.nanotime % 1_000_000_000) as u32;

        use chrono::DateTime;
        if let Some(dt) = DateTime::from_timestamp(secs, nanos) {
            dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        } else {
            "1970-01-01T00:00:00.000Z".to_string()
        }
    }
}

/// Query most recent N messages (CHATHISTORY LATEST)
/// IRCv3 SPEC: Returns newest messages in chronological order
pub async fn query_latest(db: &Database, target: &str, limit: usize) -> Result<Vec<StoredMessage>> {
    let target = target.to_string();
    let limit = limit as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let messages = conn
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT msgid, target, sender, message_data, nanotime, account, created_at
             FROM message_history
             WHERE target = ?1
             ORDER BY nanotime DESC
             LIMIT ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![target, limit], |row| {
                Ok(StoredMessage {
                    msgid: row.get(0)?,
                    target: row.get(1)?,
                    sender: row.get(2)?,
                    message_data: String::from_utf8(row.get::<_, Vec<u8>>(3)?).unwrap_or_default(),
                    nanotime: row.get(4)?,
                    account: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            // Reverse to chronological order (oldest first)
            results.reverse();
            Ok::<_, anyhow::Error>(results)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(messages)
}

/// Query messages before timestamp (CHATHISTORY BEFORE)
pub async fn query_before(
    db: &Database,
    target: &str,
    before_nanos: u128,
    limit: usize,
) -> Result<Vec<StoredMessage>> {
    let target = target.to_string();
    let before_nanos = before_nanos as i64;
    let limit = limit as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let messages = conn
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT msgid, target, sender, message_data, nanotime, account, created_at
             FROM message_history
             WHERE target = ?1 AND nanotime < ?2
             ORDER BY nanotime DESC
             LIMIT ?3",
            )?;

            let rows = stmt.query_map(rusqlite::params![target, before_nanos, limit], |row| {
                Ok(StoredMessage {
                    msgid: row.get(0)?,
                    target: row.get(1)?,
                    sender: row.get(2)?,
                    message_data: String::from_utf8(row.get::<_, Vec<u8>>(3)?).unwrap_or_default(),
                    nanotime: row.get(4)?,
                    account: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            results.reverse();
            Ok::<_, anyhow::Error>(results)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(messages)
}

/// Query messages after timestamp (CHATHISTORY AFTER)
pub async fn query_after(
    db: &Database,
    target: &str,
    after_nanos: u128,
    limit: usize,
) -> Result<Vec<StoredMessage>> {
    let target = target.to_string();
    let after_nanos = after_nanos as i64;
    let limit = limit as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let messages = conn
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT msgid, target, sender, message_data, nanotime, account, created_at
             FROM message_history
             WHERE target = ?1 AND nanotime > ?2
             ORDER BY nanotime ASC
             LIMIT ?3",
            )?;

            let rows = stmt.query_map(rusqlite::params![target, after_nanos, limit], |row| {
                Ok(StoredMessage {
                    msgid: row.get(0)?,
                    target: row.get(1)?,
                    sender: row.get(2)?,
                    message_data: String::from_utf8(row.get::<_, Vec<u8>>(3)?).unwrap_or_default(),
                    nanotime: row.get(4)?,
                    account: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(messages)
}

/// Query oldest messages (CHATHISTORY AFTER *)
pub async fn query_oldest(db: &Database, target: &str, limit: usize) -> Result<Vec<StoredMessage>> {
    let target = target.to_string();
    let limit = limit as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let messages = conn
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT msgid, target, sender, message_data, nanotime, account, created_at
             FROM message_history
             WHERE target = ?1
             ORDER BY nanotime ASC
             LIMIT ?2",
            )?;

            let rows = stmt.query_map(rusqlite::params![target, limit], |row| {
                Ok(StoredMessage {
                    msgid: row.get(0)?,
                    target: row.get(1)?,
                    sender: row.get(2)?,
                    message_data: String::from_utf8(row.get::<_, Vec<u8>>(3)?).unwrap_or_default(),
                    nanotime: row.get(4)?,
                    account: row.get(5)?,
                    created_at: row.get(6)?,
                })
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(messages)
}

/// Query messages around timestamp (CHATHISTORY AROUND)
pub async fn query_around(
    db: &Database,
    target: &str,
    around_nanos: u128,
    limit: usize,
) -> Result<Vec<StoredMessage>> {
    let half_limit = limit / 2;

    // Query before
    let mut before = query_before(db, target, around_nanos, half_limit).await?;

    // Query after
    let after = query_after(db, target, around_nanos, half_limit).await?;

    // Merge in chronological order
    before.extend(after);
    Ok(before)
}

/// Query messages between two timestamps (CHATHISTORY BETWEEN)
pub async fn query_between(
    db: &Database,
    target: &str,
    start_nanos: u128,
    end_nanos: u128,
    limit: usize,
) -> Result<Vec<StoredMessage>> {
    let target = target.to_string();
    let start_nanos = start_nanos as i64;
    let end_nanos = end_nanos as i64;
    let limit = limit as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let messages = conn
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT msgid, target, sender, message_data, nanotime, account, created_at
             FROM message_history
             WHERE target = ?1 AND nanotime > ?2 AND nanotime < ?3
             ORDER BY nanotime ASC
             LIMIT ?4",
            )?;

            let rows = stmt.query_map(
                rusqlite::params![target, start_nanos, end_nanos, limit],
                |row| {
                    Ok(StoredMessage {
                        msgid: row.get(0)?,
                        target: row.get(1)?,
                        sender: row.get(2)?,
                        message_data: String::from_utf8(row.get::<_, Vec<u8>>(3)?)
                            .unwrap_or_default(),
                        nanotime: row.get(4)?,
                        account: row.get(5)?,
                        created_at: row.get(6)?,
                    })
                },
            )?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(messages)
}

/// Query distinct targets with activity in range (CHATHISTORY TARGETS)
pub async fn query_targets(
    db: &Database,
    start_nanos: u128,
    end_nanos: u128,
    limit: usize,
) -> Result<Vec<String>> {
    let start_nanos = start_nanos as i64;
    let end_nanos = end_nanos as i64;
    let limit = limit as i64;

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let targets = conn
        .interact(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT DISTINCT target
             FROM message_history
             WHERE nanotime > ?1 AND nanotime < ?2
             ORDER BY nanotime DESC
             LIMIT ?3",
            )?;

            let rows = stmt.query_map(rusqlite::params![start_nanos, end_nanos, limit], |row| {
                row.get(0)
            })?;

            let mut results = Vec::new();
            for row in rows {
                results.push(row?);
            }

            Ok::<_, anyhow::Error>(results)
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(targets)
}

/// Lookup msgid and return its nanotime
pub async fn lookup_msgid_nanotime(
    db: &Database,
    target: &str,
    msgid: &str,
) -> Result<Option<u128>> {
    let target = target.to_string();
    let msgid = msgid.to_string();

    let conn = db
        .pool()
        .get()
        .await
        .context("getting database connection")?;

    let nanotime = conn
        .interact(move |conn| {
            let mut stmt = conn
                .prepare("SELECT nanotime FROM message_history WHERE target = ?1 AND msgid = ?2")?;

            let mut rows = stmt.query(rusqlite::params![target, msgid])?;
            let result: Option<i64> = if let Some(row) = rows.next()? {
                Some(row.get(0)?)
            } else {
                None
            };

            Ok::<_, anyhow::Error>(result.map(|n| n as u128))
        })
        .await
        .map_err(|e| anyhow::anyhow!("database interaction failed: {}", e))??;

    Ok(nanotime)
}
