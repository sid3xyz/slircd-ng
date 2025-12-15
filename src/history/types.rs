//! Type definitions for message history.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

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

/// Stored message retrieved from database.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}
