//! CTCP (Client-to-Client Protocol) message handling.
//!
//! This module provides utilities for parsing and creating CTCP messages,
//! which are embedded within PRIVMSG and NOTICE commands using the
//! `\x01` delimiter character.
//!
//! # Reference
//! - CTCP specification: <https://modern.ircdocs.horse/ctcp.html>
//!
//! # Example
//!
//! ```
//! use slirc_proto::ctcp::{Ctcp, CtcpKind};
//!
//! // Parse a CTCP message
//! let ctcp = Ctcp::parse("\x01ACTION waves hello\x01").unwrap();
//! assert_eq!(ctcp.kind, CtcpKind::Action);
//! assert_eq!(ctcp.params, Some("waves hello"));
//!
//! // Create a CTCP message
//! let action = Ctcp::action("dances");
//! assert_eq!(action.to_string(), "\x01ACTION dances\x01");
//! ```

use std::fmt;

/// The CTCP delimiter character (`\x01`).
pub(crate) const CTCP_DELIM: char = '\x01';

/// Known CTCP command types.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CtcpKind {
    /// ACTION - describes an action performed by the user.
    /// Commonly triggered by `/me` in IRC clients.
    Action,
    /// VERSION - requests client version information.
    Version,
    /// PING - measures round-trip latency.
    Ping,
    /// TIME - requests local time from the client.
    Time,
    /// USERINFO - requests user-defined information.
    Userinfo,
    /// CLIENTINFO - requests list of supported CTCP commands.
    Clientinfo,
    /// SOURCE - requests source code location.
    Source,
    /// FINGER - requests user information (legacy).
    Finger,
    /// DCC - Direct Client-to-Client connection setup.
    Dcc,
    /// Unknown or custom CTCP command.
    Unknown(String),
}

impl CtcpKind {
    /// Parse a CTCP command name into a `CtcpKind`.
    pub fn parse(name: &str) -> Self {
        match name.to_ascii_uppercase().as_str() {
            "ACTION" => Self::Action,
            "VERSION" => Self::Version,
            "PING" => Self::Ping,
            "TIME" => Self::Time,
            "USERINFO" => Self::Userinfo,
            "CLIENTINFO" => Self::Clientinfo,
            "SOURCE" => Self::Source,
            "FINGER" => Self::Finger,
            "DCC" => Self::Dcc,
            _ => Self::Unknown(name.to_owned()),
        }
    }

    /// Returns the canonical uppercase name of this CTCP command.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Action => "ACTION",
            Self::Version => "VERSION",
            Self::Ping => "PING",
            Self::Time => "TIME",
            Self::Userinfo => "USERINFO",
            Self::Clientinfo => "CLIENTINFO",
            Self::Source => "SOURCE",
            Self::Finger => "FINGER",
            Self::Dcc => "DCC",
            Self::Unknown(s) => s,
        }
    }
}

impl fmt::Display for CtcpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A parsed CTCP message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ctcp<'a> {
    /// The CTCP command type.
    pub kind: CtcpKind,
    /// Optional parameters following the command.
    pub params: Option<&'a str>,
}

impl<'a> Ctcp<'a> {
    /// Parse a CTCP message from a PRIVMSG/NOTICE body.
    ///
    /// Returns `None` if the message is not a valid CTCP message.
    ///
    /// # Example
    ///
    /// ```
    /// use slirc_proto::ctcp::Ctcp;
    ///
    /// let ctcp = Ctcp::parse("\x01VERSION\x01").unwrap();
    /// assert_eq!(ctcp.kind.as_str(), "VERSION");
    /// assert_eq!(ctcp.params, None);
    /// ```
    pub fn parse(text: &'a str) -> Option<Self> {
        // Must start and end with CTCP delimiter
        let text = text.strip_prefix(CTCP_DELIM)?;
        let text = text.strip_suffix(CTCP_DELIM).unwrap_or(text);

        if text.is_empty() {
            return None;
        }

        // Split into command and optional params
        let (command, params) = match text.find(' ') {
            Some(pos) => {
                let params = &text[pos + 1..];
                (
                    &text[..pos],
                    if params.is_empty() {
                        None
                    } else {
                        Some(params)
                    },
                )
            }
            None => (text, None),
        };

        Some(Self {
            kind: CtcpKind::parse(command),
            params,
        })
    }

    /// Check if a message body contains a CTCP message.
    #[inline]
    pub fn is_ctcp(text: &str) -> bool {
        text.starts_with(CTCP_DELIM)
    }

    /// Create an ACTION CTCP message.
    ///
    /// # Example
    ///
    /// ```
    /// use slirc_proto::ctcp::Ctcp;
    ///
    /// let action = Ctcp::action("waves");
    /// assert_eq!(action.to_string(), "\x01ACTION waves\x01");
    /// ```
    pub fn action(text: &'a str) -> Self {
        Self {
            kind: CtcpKind::Action,
            params: Some(text),
        }
    }

    /// Create a VERSION request.
    pub fn version() -> Self {
        Self {
            kind: CtcpKind::Version,
            params: None,
        }
    }

    /// Create a VERSION reply.
    pub fn version_reply(version: &'a str) -> Self {
        Self {
            kind: CtcpKind::Version,
            params: Some(version),
        }
    }

    /// Create a PING request with a timestamp.
    pub fn ping(timestamp: &'a str) -> Self {
        Self {
            kind: CtcpKind::Ping,
            params: Some(timestamp),
        }
    }

    /// Create a TIME reply.
    pub fn time_reply(time: &'a str) -> Self {
        Self {
            kind: CtcpKind::Time,
            params: Some(time),
        }
    }

    /// Create a CLIENTINFO reply listing supported commands.
    pub fn clientinfo_reply(commands: &'a str) -> Self {
        Self {
            kind: CtcpKind::Clientinfo,
            params: Some(commands),
        }
    }

    /// Create a custom CTCP message.
    pub fn custom(command: &str, params: Option<&'a str>) -> Self {
        Self {
            kind: CtcpKind::parse(command),
            params,
        }
    }
}

impl fmt::Display for Ctcp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x01{}", self.kind)?;
        if let Some(params) = self.params {
            write!(f, " {}", params)?;
        }
        write!(f, "\x01")
    }
}

/// An owned version of `Ctcp` for when lifetime management is needed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtcpOwned {
    /// The CTCP command type.
    pub kind: CtcpKind,
    /// Optional parameters following the command.
    pub params: Option<String>,
}

impl CtcpOwned {
    /// Parse a CTCP message from a PRIVMSG/NOTICE body.
    pub fn parse(text: &str) -> Option<Self> {
        Ctcp::parse(text).map(|c| Self {
            kind: c.kind,
            params: c.params.map(|s| s.to_owned()),
        })
    }

    /// Create an ACTION CTCP message.
    pub fn action(text: impl Into<String>) -> Self {
        Self {
            kind: CtcpKind::Action,
            params: Some(text.into()),
        }
    }

    /// Create a VERSION request.
    pub fn version() -> Self {
        Self {
            kind: CtcpKind::Version,
            params: None,
        }
    }

    /// Create a VERSION reply.
    pub fn version_reply(version: impl Into<String>) -> Self {
        Self {
            kind: CtcpKind::Version,
            params: Some(version.into()),
        }
    }

    /// Create a PING request with a timestamp.
    pub fn ping(timestamp: impl Into<String>) -> Self {
        Self {
            kind: CtcpKind::Ping,
            params: Some(timestamp.into()),
        }
    }
}

impl fmt::Display for CtcpOwned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\x01{}", self.kind)?;
        if let Some(ref params) = self.params {
            write!(f, " {}", params)?;
        }
        write!(f, "\x01")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_action() {
        let ctcp = Ctcp::parse("\x01ACTION waves hello\x01").unwrap();
        assert_eq!(ctcp.kind, CtcpKind::Action);
        assert_eq!(ctcp.params, Some("waves hello"));
    }

    #[test]
    fn test_parse_version() {
        let ctcp = Ctcp::parse("\x01VERSION\x01").unwrap();
        assert_eq!(ctcp.kind, CtcpKind::Version);
        assert_eq!(ctcp.params, None);
    }

    #[test]
    fn test_parse_version_reply() {
        let ctcp = Ctcp::parse("\x01VERSION irssi 1.2.3\x01").unwrap();
        assert_eq!(ctcp.kind, CtcpKind::Version);
        assert_eq!(ctcp.params, Some("irssi 1.2.3"));
    }

    #[test]
    fn test_parse_ping() {
        let ctcp = Ctcp::parse("\x01PING 1234567890\x01").unwrap();
        assert_eq!(ctcp.kind, CtcpKind::Ping);
        assert_eq!(ctcp.params, Some("1234567890"));
    }

    #[test]
    fn test_parse_unknown() {
        let ctcp = Ctcp::parse("\x01CUSTOM foo bar\x01").unwrap();
        assert_eq!(ctcp.kind, CtcpKind::Unknown("CUSTOM".to_owned()));
        assert_eq!(ctcp.params, Some("foo bar"));
    }

    #[test]
    fn test_parse_case_insensitive() {
        let ctcp = Ctcp::parse("\x01action waves\x01").unwrap();
        assert_eq!(ctcp.kind, CtcpKind::Action);
    }

    #[test]
    fn test_parse_missing_trailing_delim() {
        // Some clients omit the trailing delimiter
        let ctcp = Ctcp::parse("\x01ACTION waves").unwrap();
        assert_eq!(ctcp.kind, CtcpKind::Action);
        assert_eq!(ctcp.params, Some("waves"));
    }

    #[test]
    fn test_parse_not_ctcp() {
        assert!(Ctcp::parse("hello world").is_none());
        assert!(Ctcp::parse("").is_none());
        assert!(Ctcp::parse("\x01\x01").is_none());
    }

    #[test]
    fn test_is_ctcp() {
        assert!(Ctcp::is_ctcp("\x01ACTION waves\x01"));
        assert!(Ctcp::is_ctcp("\x01VERSION\x01"));
        assert!(!Ctcp::is_ctcp("hello world"));
    }

    #[test]
    fn test_action_display() {
        let action = Ctcp::action("dances");
        assert_eq!(action.to_string(), "\x01ACTION dances\x01");
    }

    #[test]
    fn test_version_display() {
        let version = Ctcp::version();
        assert_eq!(version.to_string(), "\x01VERSION\x01");
    }

    #[test]
    fn test_ping_display() {
        let ping = Ctcp::ping("12345");
        assert_eq!(ping.to_string(), "\x01PING 12345\x01");
    }

    #[test]
    fn test_owned_action() {
        let action = CtcpOwned::action("waves");
        assert_eq!(action.to_string(), "\x01ACTION waves\x01");
    }

    #[test]
    fn test_roundtrip() {
        let original = "\x01ACTION does something\x01";
        let parsed = Ctcp::parse(original).unwrap();
        assert_eq!(parsed.to_string(), original);
    }

    #[test]
    fn test_all_kinds() {
        assert_eq!(CtcpKind::Action.as_str(), "ACTION");
        assert_eq!(CtcpKind::Version.as_str(), "VERSION");
        assert_eq!(CtcpKind::Ping.as_str(), "PING");
        assert_eq!(CtcpKind::Time.as_str(), "TIME");
        assert_eq!(CtcpKind::Userinfo.as_str(), "USERINFO");
        assert_eq!(CtcpKind::Clientinfo.as_str(), "CLIENTINFO");
        assert_eq!(CtcpKind::Source.as_str(), "SOURCE");
        assert_eq!(CtcpKind::Finger.as_str(), "FINGER");
        assert_eq!(CtcpKind::Dcc.as_str(), "DCC");
    }
}
