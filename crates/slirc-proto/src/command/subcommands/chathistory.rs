//! CHATHISTORY subcommand types for IRCv3 chat history retrieval.
//!
//! # Reference
//! - IRCv3 chathistory specification: <https://ircv3.net/specs/extensions/chathistory>

use std::str::FromStr;

use crate::error::MessageParseError;

/// Subcommand for CHATHISTORY messages.
///
/// CHATHISTORY is used to request message history from the server.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum ChatHistorySubCommand {
    /// Request the latest messages: `LATEST <target> <* | msgref> <limit>`
    LATEST,
    /// Request messages before a point: `BEFORE <target> <msgref> <limit>`
    BEFORE,
    /// Request messages after a point: `AFTER <target> <msgref> <limit>`
    AFTER,
    /// Request messages around a point: `AROUND <target> <msgref> <limit>`
    AROUND,
    /// Request messages between two points: `BETWEEN <target> <msgref> <msgref> <limit>`
    BETWEEN,
    /// List channels/users with history: `TARGETS <timestamp> <timestamp> <limit>`
    TARGETS,
}

impl ChatHistorySubCommand {
    /// Get the string representation of this subcommand.
    pub fn as_str(&self) -> &str {
        match self {
            Self::LATEST => "LATEST",
            Self::BEFORE => "BEFORE",
            Self::AFTER => "AFTER",
            Self::AROUND => "AROUND",
            Self::BETWEEN => "BETWEEN",
            Self::TARGETS => "TARGETS",
        }
    }
}

impl FromStr for ChatHistorySubCommand {
    type Err = MessageParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_uppercase().as_str() {
            "LATEST" => Ok(Self::LATEST),
            "BEFORE" => Ok(Self::BEFORE),
            "AFTER" => Ok(Self::AFTER),
            "AROUND" => Ok(Self::AROUND),
            "BETWEEN" => Ok(Self::BETWEEN),
            "TARGETS" => Ok(Self::TARGETS),
            _ => Err(MessageParseError::InvalidSubcommand {
                cmd: "CHATHISTORY",
                sub: s.to_owned(),
            }),
        }
    }
}

impl std::fmt::Display for ChatHistorySubCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A message reference for CHATHISTORY commands.
///
/// Can be either a timestamp or a message ID.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum MessageReference {
    /// Timestamp reference: `timestamp=YYYY-MM-DDThh:mm:ss.sssZ`
    Timestamp(String),
    /// Message ID reference: `msgid=abc123`
    MsgId(String),
    /// Wildcard `*` (only valid for LATEST)
    Wildcard,
}

impl MessageReference {
    /// Parse a message reference from a string.
    ///
    /// Accepts formats:
    /// - `timestamp=2023-01-01T12:00:00.000Z`
    /// - `msgid=abc123`
    /// - `*` (wildcard)
    pub fn parse(s: &str) -> Result<Self, MessageParseError> {
        if s == "*" {
            return Ok(Self::Wildcard);
        }
        if let Some(ts) = s.strip_prefix("timestamp=") {
            return Ok(Self::Timestamp(ts.to_owned()));
        }
        if let Some(id) = s.strip_prefix("msgid=") {
            return Ok(Self::MsgId(id.to_owned()));
        }
        Err(MessageParseError::InvalidArgument(format!(
            "invalid message reference: {s}"
        )))
    }
}

impl std::fmt::Display for MessageReference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timestamp(ts) => write!(f, "timestamp={ts}"),
            Self::MsgId(id) => write!(f, "msgid={id}"),
            Self::Wildcard => f.write_str("*"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_subcommand_parse() {
        assert_eq!(
            "LATEST".parse::<ChatHistorySubCommand>().unwrap(),
            ChatHistorySubCommand::LATEST
        );
        assert_eq!(
            "before".parse::<ChatHistorySubCommand>().unwrap(),
            ChatHistorySubCommand::BEFORE
        );
        assert_eq!(
            "TARGETS".parse::<ChatHistorySubCommand>().unwrap(),
            ChatHistorySubCommand::TARGETS
        );
        assert!("UNKNOWN".parse::<ChatHistorySubCommand>().is_err());
    }

    #[test]
    fn test_subcommand_display() {
        assert_eq!(format!("{}", ChatHistorySubCommand::LATEST), "LATEST");
        assert_eq!(format!("{}", ChatHistorySubCommand::BETWEEN), "BETWEEN");
    }

    #[test]
    fn test_msgref_parse() {
        assert_eq!(
            MessageReference::parse("*").unwrap(),
            MessageReference::Wildcard
        );
        assert_eq!(
            MessageReference::parse("timestamp=2023-01-01T12:00:00.000Z").unwrap(),
            MessageReference::Timestamp("2023-01-01T12:00:00.000Z".to_string())
        );
        assert_eq!(
            MessageReference::parse("msgid=abc123").unwrap(),
            MessageReference::MsgId("abc123".to_string())
        );
        assert!(MessageReference::parse("invalid").is_err());
    }

    #[test]
    fn test_msgref_display() {
        assert_eq!(format!("{}", MessageReference::Wildcard), "*");
        assert_eq!(
            format!(
                "{}",
                MessageReference::Timestamp("2023-01-01T12:00:00Z".to_string())
            ),
            "timestamp=2023-01-01T12:00:00Z"
        );
        assert_eq!(
            format!("{}", MessageReference::MsgId("abc123".to_string())),
            "msgid=abc123"
        );
    }
}
