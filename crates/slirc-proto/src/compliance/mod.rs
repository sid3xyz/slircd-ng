//! RFC compliance checking utilities.
//!
//! This module provides tools to validate IRC messages against RFC 1459, RFC 2812,
//! and IRCv3 specifications.
//!
//! # Example
//!
//! ```
//! use slirc_proto::compliance::{check_compliance, ComplianceConfig};
//! use slirc_proto::MessageRef;
//!
//! let raw = ":nick!user@host PRIVMSG #channel :Hello world!";
//! let msg = MessageRef::parse(raw).unwrap();
//! let config = ComplianceConfig::default();
//!
//! match check_compliance(&msg, Some(raw.len()), &config) {
//!     Ok(_) => println!("Message is RFC compliant"),
//!     Err(errors) => {
//!         for err in errors {
//!             println!("Compliance issue: {}", err);
//!         }
//!     }
//! }
//! ```

use crate::MessageRef;
use std::fmt;

/// Errors that can occur during compliance checking.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComplianceError {
    /// The message exceeds the maximum allowed length (512 bytes).
    LineTooLong(usize),
    /// The command is empty.
    EmptyCommand,
    /// The command contains invalid characters.
    InvalidCommand(String),
    /// A required parameter is missing for the command.
    MissingParameter(&'static str),
    /// The prefix is invalid.
    InvalidPrefix(String),
    /// The channel name is invalid.
    InvalidChannelName(String),
    /// The nickname is invalid.
    InvalidNickname(String),
    /// A parameter contains invalid characters.
    InvalidParameter(String),
}

impl fmt::Display for ComplianceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ComplianceError::LineTooLong(len) => {
                write!(f, "Message length {} exceeds 512 bytes", len)
            }
            ComplianceError::EmptyCommand => write!(f, "Command is empty"),
            ComplianceError::InvalidCommand(cmd) => write!(f, "Invalid command: {}", cmd),
            ComplianceError::MissingParameter(param) => {
                write!(f, "Missing required parameter: {}", param)
            }
            ComplianceError::InvalidPrefix(p) => write!(f, "Invalid prefix: {}", p),
            ComplianceError::InvalidChannelName(c) => write!(f, "Invalid channel name: {}", c),
            ComplianceError::InvalidNickname(n) => write!(f, "Invalid nickname: {}", n),
            ComplianceError::InvalidParameter(p) => write!(f, "Invalid parameter: {}", p),
        }
    }
}

impl std::error::Error for ComplianceError {}

/// Configuration for the compliance checker.
#[derive(Debug, Clone, Default)]
pub struct ComplianceConfig {
    /// Whether to enforce strict RFC 1459 channel naming (start with #, &, etc.).
    pub strict_channel_names: bool,
    /// Whether to enforce strict nickname format.
    pub strict_nicknames: bool,
}

/// Checks if a message complies with IRC specifications.
///
/// # Arguments
///
/// * `message` - The parsed message to check.
/// * `raw_len` - Optional length of the raw message in bytes (including CRLF).
///   If provided, checks if it exceeds 512 bytes.
/// * `config` - Configuration for the checker.
pub fn check_compliance(
    message: &MessageRef<'_>,
    raw_len: Option<usize>,
    config: &ComplianceConfig,
) -> Result<(), Vec<ComplianceError>> {
    let mut errors = Vec::new();

    // Check line length
    if let Some(len) = raw_len {
        if len > 512 {
            errors.push(ComplianceError::LineTooLong(len));
        }
    }

    // Check command
    if message.command.name.is_empty() {
        errors.push(ComplianceError::EmptyCommand);
    } else if !message
        .command
        .name
        .chars()
        .all(|c| c.is_ascii_alphabetic() || c.is_ascii_digit())
    {
        // RFC 2812 Section 2.3.1: Command is either a valid letter or a 3-digit number.
        // We allow alphanumeric for broader compatibility, but could be stricter.
        errors.push(ComplianceError::InvalidCommand(
            message.command.name.to_string(),
        ));
    }

    // Check for invalid characters in parameters
    for arg in &message.command.args {
        if arg.contains(['\0', '\r', '\n']) {
            errors.push(ComplianceError::InvalidParameter(arg.to_string()));
        }
    }

    // Command-specific checks
    match message.command.name {
        "JOIN" => {
            if message.command.args.is_empty() {
                errors.push(ComplianceError::MissingParameter("channel"));
            } else if config.strict_channel_names {
                for channel in message.command.args[0].split(',') {
                    if !is_valid_channel(channel) {
                        errors.push(ComplianceError::InvalidChannelName(channel.to_string()));
                    }
                }
            }
        }
        "PRIVMSG" | "NOTICE" => {
            if message.command.args.is_empty() {
                errors.push(ComplianceError::MissingParameter("target"));
            } else if message.command.args.len() < 2 {
                errors.push(ComplianceError::MissingParameter("text"));
            }
        }
        "NICK" => {
            if message.command.args.is_empty() {
                errors.push(ComplianceError::MissingParameter("nickname"));
            } else if config.strict_nicknames && !is_valid_nickname(message.command.args[0]) {
                errors.push(ComplianceError::InvalidNickname(
                    message.command.args[0].to_string(),
                ));
            }
        }
        _ => {}
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn is_valid_channel(channel: &str) -> bool {
    // RFC 2812: channels begin with &, #, +, or !
    // Length <= 50
    if channel.len() > 50 || channel.is_empty() {
        return false;
    }
    let first = channel.chars().next().unwrap();
    matches!(first, '&' | '#' | '+' | '!') && !channel.contains([' ', '\x07', ','])
}

fn is_valid_nickname(nick: &str) -> bool {
    // RFC 2812: ( letter / special ) *8( letter / digit / special / "-" )
    // Max length 9 usually, but often extended.
    if nick.is_empty() || nick.len() > 9 {
        // Strict RFC 2812 limit
        return false;
    }
    // Simplified check
    let mut chars = nick.chars();
    if let Some(first) = chars.next() {
        if !first.is_ascii_alphabetic() && !is_special(first) {
            return false;
        }
        for c in chars {
            if !c.is_ascii_alphanumeric() && !is_special(c) && c != '-' {
                return false;
            }
        }
        true
    } else {
        false
    }
}

fn is_special(c: char) -> bool {
    matches!(c, '[' | ']' | '\\' | '`' | '_' | '^' | '{' | '|' | '}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageRef;

    #[test]
    fn test_valid_message() {
        let raw = ":nick!user@host PRIVMSG #channel :Hello!";
        let msg = MessageRef::parse(raw).unwrap();
        let config = ComplianceConfig::default();
        assert!(check_compliance(&msg, Some(raw.len()), &config).is_ok());
    }

    #[test]
    fn test_line_too_long() {
        let raw = "PING test";
        let msg = MessageRef::parse(raw).unwrap();
        let config = ComplianceConfig::default();
        let result = check_compliance(&msg, Some(600), &config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(matches!(errors[0], ComplianceError::LineTooLong(600)));
    }

    #[test]
    fn test_missing_privmsg_params() {
        let raw = "PRIVMSG";
        let msg = MessageRef::parse(raw).unwrap();
        let config = ComplianceConfig::default();
        let result = check_compliance(&msg, None, &config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(matches!(
            errors[0],
            ComplianceError::MissingParameter("target")
        ));
    }

    #[test]
    fn test_missing_privmsg_text() {
        let raw = "PRIVMSG #channel";
        let msg = MessageRef::parse(raw).unwrap();
        let config = ComplianceConfig::default();
        let result = check_compliance(&msg, None, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_missing_join_channel() {
        let raw = "JOIN";
        let msg = MessageRef::parse(raw).unwrap();
        let config = ComplianceConfig::default();
        let result = check_compliance(&msg, None, &config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(matches!(
            errors[0],
            ComplianceError::MissingParameter("channel")
        ));
    }

    #[test]
    fn test_strict_channel_names() {
        let raw = "JOIN invalidchannel";
        let msg = MessageRef::parse(raw).unwrap();
        let config = ComplianceConfig {
            strict_channel_names: true,
            strict_nicknames: false,
        };
        let result = check_compliance(&msg, None, &config);
        assert!(result.is_err());
        let errors = result.unwrap_err();
        assert!(matches!(errors[0], ComplianceError::InvalidChannelName(_)));
    }

    #[test]
    fn test_valid_channel_names() {
        for channel in ["#test", "&local", "+modeless", "!12345test"] {
            assert!(is_valid_channel(channel), "Expected valid: {}", channel);
        }
    }

    #[test]
    fn test_invalid_channel_names() {
        for channel in ["", "nochanprefix", "#has space", "#has,comma"] {
            assert!(!is_valid_channel(channel), "Expected invalid: {}", channel);
        }
    }

    #[test]
    fn test_strict_nicknames() {
        let raw = "NICK 123invalid";
        let msg = MessageRef::parse(raw).unwrap();
        let config = ComplianceConfig {
            strict_channel_names: false,
            strict_nicknames: true,
        };
        let result = check_compliance(&msg, None, &config);
        assert!(result.is_err());
    }

    #[test]
    fn test_valid_nicknames() {
        for nick in ["Nick", "nick123", "n1ck", "Nick_", "[nick]"] {
            assert!(is_valid_nickname(nick), "Expected valid: {}", nick);
        }
    }

    #[test]
    fn test_invalid_nicknames() {
        for nick in ["", "123nick", "verylongnickname", "nick space"] {
            assert!(!is_valid_nickname(nick), "Expected invalid: {}", nick);
        }
    }

    #[test]
    fn test_error_display() {
        let err = ComplianceError::LineTooLong(600);
        assert!(err.to_string().contains("600"));

        let err = ComplianceError::MissingParameter("target");
        assert!(err.to_string().contains("target"));
    }

    #[test]
    fn test_config_default() {
        let config = ComplianceConfig::default();
        assert!(!config.strict_channel_names);
        assert!(!config.strict_nicknames);
    }
}
