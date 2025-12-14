//! Type definitions for message history.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Row type from database query: (msgid, target, sender, message_data, nanotime, account)
pub(super) type HistoryRow = (String, String, String, Vec<u8>, i64, Option<String>);

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
    /// Selected from DB for struct completeness; envelope.target has display name.
    #[allow(dead_code)] // DB field - accessed via envelope.target instead
    pub target: String,
    /// Sender nickname.
    /// Selected from DB for struct completeness; envelope.prefix has full sender.
    #[allow(dead_code)] // DB field - accessed via envelope.prefix instead
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

    /// Parse a database row into a StoredMessage.
    pub(super) fn from_row(row: HistoryRow) -> Option<Self> {
        let (msgid, target, sender, data, nanotime, account) = row;
        let envelope: MessageEnvelope = serde_json::from_slice(&data).ok()?;
        Some(StoredMessage {
            msgid,
            target,
            sender,
            envelope,
            nanotime,
            account,
        })
    }
}

/// Convert database rows to StoredMessages, optionally reversing for chronological order.
pub(super) fn rows_to_messages(rows: Vec<HistoryRow>, reverse: bool) -> Vec<StoredMessage> {
    let mut messages: Vec<StoredMessage> = rows
        .into_iter()
        .filter_map(StoredMessage::from_row)
        .collect();
    if reverse {
        messages.reverse();
    }
    messages
}
