//! IRCv3 capability negotiation support.
//!
//! This module provides types and utilities for IRCv3 capability negotiation,
//! allowing clients and servers to negotiate optional protocol extensions.
//!
//! # Reference
//! - IRCv3 Capability Negotiation: <https://ircv3.net/specs/extensions/capability-negotiation>
//! - Individual capability specifications: <https://ircv3.net/irc/>

mod negotiation;
mod registry;

pub use negotiation::{apply_changes, format_cap_del, format_cap_new, parse_request};
pub use registry::{get_all_names, get_cap_list, is_supported, CapabilityDef, CAPABILITIES};

/// Known IRCv3 capability types.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Capability {
    /// Show all user prefix modes in NAMES
    MultiPrefix,
    /// SASL authentication
    Sasl,
    /// Notify of account login/logout
    AccountNotify,
    /// Notify of away status changes
    AwayNotify,
    /// Extended JOIN with account and realname
    ExtendedJoin,
    /// MONITOR command for presence tracking
    Monitor,
    /// Add account tag to messages
    AccountTag,
    /// Message batching
    Batch,
    /// Notify of capability changes
    CapNotify,
    /// Notify of hostname changes
    ChgHost,
    /// Echo messages back to sender
    EchoMessage,
    /// Notify of channel invites
    InviteNotify,
    /// Server-time message tags
    ServerTime,
    /// Full nick!user@host in NAMES
    UserhostInNames,
    /// SETNAME command for changing realname
    SetName,
    /// Client message tags support
    MessageTags,
    /// Unique message IDs
    Msgid,
    /// Label request/response correlation
    LabeledResponse,
    /// FAIL/WARN/NOTE standard replies
    StandardReplies,
    /// Strict Transport Security
    Sts,
    /// STARTTLS upgrade capability
    Tls,
    // Draft/experimental capabilities
    /// Chat history retrieval (draft/chathistory)
    ChatHistory,
    /// Account registration (draft/account-registration)
    AccountRegistration,
    /// Multi-line messages (draft/multiline)
    Multiline,
    /// Read marker synchronization (draft/read-marker)
    ReadMarker,
    /// Typing notifications
    Typing,
    /// Event playback for history (draft/event-playback)
    EventPlayback,
    /// Message redaction/deletion (draft/message-redaction)
    MessageRedaction,
    /// Extended MONITOR notifications (extended-monitor)
    ExtendedMonitor,
    /// Unknown/custom capability
    Custom(String),
}

impl AsRef<str> for Capability {
    fn as_ref(&self) -> &str {
        match self {
            Self::MultiPrefix => "multi-prefix",
            Self::Sasl => "sasl",
            Self::AccountNotify => "account-notify",
            Self::AwayNotify => "away-notify",
            Self::ExtendedJoin => "extended-join",
            Self::Monitor => "monitor",
            Self::AccountTag => "account-tag",
            Self::Batch => "batch",
            Self::CapNotify => "cap-notify",
            Self::ChgHost => "chghost",
            Self::EchoMessage => "echo-message",
            Self::InviteNotify => "invite-notify",
            Self::ServerTime => "server-time",
            Self::UserhostInNames => "userhost-in-names",
            Self::SetName => "setname",
            Self::MessageTags => "message-tags",
            Self::Msgid => "msgid",
            Self::LabeledResponse => "labeled-response",
            Self::StandardReplies => "standard-replies",
            Self::Sts => "sts",
            Self::Tls => "tls",
            Self::ChatHistory => "draft/chathistory",
            Self::AccountRegistration => "draft/account-registration",
            Self::Multiline => "draft/multiline",
            Self::ReadMarker => "draft/read-marker",
            Self::Typing => "typing",
            Self::EventPlayback => "draft/event-playback",
            Self::MessageRedaction => "draft/message-redaction",
            Self::ExtendedMonitor => "extended-monitor",
            Self::Custom(s) => s,
        }
    }
}

impl std::fmt::Display for Capability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_ref())
    }
}

impl From<&str> for Capability {
    fn from(s: &str) -> Self {
        match s {
            "multi-prefix" => Self::MultiPrefix,
            "sasl" => Self::Sasl,
            "account-notify" => Self::AccountNotify,
            "away-notify" => Self::AwayNotify,
            "extended-join" => Self::ExtendedJoin,
            "monitor" => Self::Monitor,
            "account-tag" => Self::AccountTag,
            "batch" => Self::Batch,
            "cap-notify" => Self::CapNotify,
            "chghost" => Self::ChgHost,
            "echo-message" => Self::EchoMessage,
            "invite-notify" => Self::InviteNotify,
            "server-time" => Self::ServerTime,
            "userhost-in-names" => Self::UserhostInNames,
            "setname" => Self::SetName,
            "message-tags" => Self::MessageTags,
            "msgid" => Self::Msgid,
            "labeled-response" => Self::LabeledResponse,
            "standard-replies" => Self::StandardReplies,
            "sts" => Self::Sts,
            "tls" => Self::Tls,
            "draft/chathistory" => Self::ChatHistory,
            "draft/multiline" => Self::Multiline,
            "draft/read-marker" => Self::ReadMarker,
            "typing" => Self::Typing,
            "draft/event-playback" => Self::EventPlayback,
            "draft/message-redaction" => Self::MessageRedaction,
            "extended-monitor" => Self::ExtendedMonitor,
            other => Self::Custom(other.to_string()),
        }
    }
}

/// CAP negotiation version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegotiationVersion {
    /// CAP 3.1
    V301,
    /// CAP 3.2
    V302,
}

impl NegotiationVersion {
    /// Get the numeric version value.
    pub fn version(&self) -> u32 {
        match self {
            Self::V301 => 301,
            Self::V302 => 302,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_as_ref() {
        assert_eq!(Capability::MultiPrefix.as_ref(), "multi-prefix");
        assert_eq!(Capability::Sasl.as_ref(), "sasl");
    }

    #[test]
    fn test_capability_from_str() {
        assert_eq!(Capability::from("multi-prefix"), Capability::MultiPrefix);
        assert_eq!(Capability::from("sasl"), Capability::Sasl);
        assert_eq!(
            Capability::from("unknown-cap"),
            Capability::Custom("unknown-cap".to_string())
        );
    }

    #[test]
    fn test_draft_capabilities() {
        assert_eq!(Capability::ChatHistory.as_ref(), "draft/chathistory");
        assert_eq!(Capability::Multiline.as_ref(), "draft/multiline");
        assert_eq!(Capability::ReadMarker.as_ref(), "draft/read-marker");
        assert_eq!(Capability::Typing.as_ref(), "typing");
        assert_eq!(Capability::EventPlayback.as_ref(), "draft/event-playback");
        assert_eq!(
            Capability::MessageRedaction.as_ref(),
            "draft/message-redaction"
        );
    }

    #[test]
    fn test_draft_capabilities_from_str() {
        assert_eq!(
            Capability::from("draft/chathistory"),
            Capability::ChatHistory
        );
        assert_eq!(Capability::from("draft/multiline"), Capability::Multiline);
        assert_eq!(
            Capability::from("draft/read-marker"),
            Capability::ReadMarker
        );
        assert_eq!(Capability::from("typing"), Capability::Typing);
        assert_eq!(
            Capability::from("draft/event-playback"),
            Capability::EventPlayback
        );
        assert_eq!(
            Capability::from("draft/message-redaction"),
            Capability::MessageRedaction
        );
    }
}
