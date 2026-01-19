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
    pub target: String,
    /// Sender nickname.
    /// Selected from DB for struct completeness; envelope.prefix has full sender.
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to build a minimal StoredMessage for testing timestamp_iso().
    fn make_message(nanotime: i64) -> StoredMessage {
        StoredMessage {
            msgid: "test-msgid".to_string(),
            target: "#test".to_string(),
            sender: "testnick".to_string(),
            envelope: MessageEnvelope {
                command: "PRIVMSG".to_string(),
                prefix: "testnick!user@host".to_string(),
                target: "#test".to_string(),
                text: "test message".to_string(),
                tags: None,
            },
            nanotime,
            account: None,
        }
    }

    #[test]
    fn test_timestamp_iso_normal() {
        // 2023-11-14T22:13:20.000Z in nanoseconds
        let msg = make_message(1_700_000_000_000_000_000);
        let iso = msg.timestamp_iso();
        assert_eq!(iso, "2023-11-14T22:13:20.000Z");
    }

    #[test]
    fn test_timestamp_iso_zero() {
        // Unix epoch should produce 1970-01-01T00:00:00.000Z
        let msg = make_message(0);
        let iso = msg.timestamp_iso();
        assert_eq!(iso, "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_timestamp_iso_with_milliseconds() {
        // 1_700_000_000_123_000_000 ns = 1700000000.123 seconds
        // Should show .123 milliseconds
        let msg = make_message(1_700_000_000_123_000_000);
        let iso = msg.timestamp_iso();
        assert_eq!(iso, "2023-11-14T22:13:20.123Z");
    }

    #[test]
    fn test_timestamp_iso_subsecond_precision() {
        // 500 milliseconds = 500_000_000 nanoseconds
        let msg = make_message(500_000_000);
        let iso = msg.timestamp_iso();
        assert_eq!(iso, "1970-01-01T00:00:00.500Z");
    }

    #[test]
    fn test_timestamp_iso_negative_fallback() {
        // Negative timestamp should fallback to epoch
        // chrono::DateTime::from_timestamp returns None for invalid values
        let msg = make_message(i64::MIN);
        let iso = msg.timestamp_iso();
        assert_eq!(iso, "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn test_timestamp_iso_one_second() {
        // Exactly 1 second after epoch
        let msg = make_message(1_000_000_000);
        let iso = msg.timestamp_iso();
        assert_eq!(iso, "1970-01-01T00:00:01.000Z");
    }
}

// =================================================================================
// EventPlayback Types
// =================================================================================

/// Unified history item (message or event).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HistoryItem {
    Message(StoredMessage),
    Event(StoredEvent),
}

impl HistoryItem {
    /// Get the timestamp (nanoseconds) for sorting.
    pub fn nanotime(&self) -> i64 {
        match self {
            Self::Message(m) => m.nanotime,
            Self::Event(e) => e.nanotime,
        }
    }
}

/// Stored protocol event (JOIN, PART, MODE, etc.).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: String, // UUID
    pub nanotime: i64,
    pub source: String, // Nick!User@Host
    pub kind: EventKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventKind {
    Join,
    Part(Option<String>), // Reason
    Quit(Option<String>), // Reason
    Kick {
        target: String,
        reason: Option<String>,
    },
    Mode {
        diff: String,
    }, // e.g., "+o user"
    Topic {
        old_topic: Option<String>,
        new_topic: String,
    },
    Nick {
        new_nick: String,
    },
}
