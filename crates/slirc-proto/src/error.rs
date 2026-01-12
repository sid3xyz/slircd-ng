//! Error types for the IRC protocol library.
//!
//! This module defines error types for protocol-level errors,
//! message parsing failures, and mode parsing issues.

use thiserror::Error;

/// Convenience type alias for Results using [`ProtocolError`].
pub type Result<T, E = ProtocolError> = std::result::Result<T, E>;

/// Extract the IRC command name from raw message bytes (for error reporting).
///
/// This function attempts to parse the command from raw bytes without validating UTF-8,
/// allowing extraction of the command name even when the message contains invalid UTF-8.
///
/// The command is the first space-delimited token after optional tags (@...) and prefix (:...).
///
/// # Examples
///
/// ```ignore
/// assert_eq!(extract_command_hint(b"PRIVMSG #test hi"), Some("PRIVMSG".to_string()));
/// assert_eq!(extract_command_hint(b":server PRIVMSG #test hi"), Some("PRIVMSG".to_string()));
/// assert_eq!(extract_command_hint(b"@label=xyz PRIVMSG #test hi"), Some("PRIVMSG".to_string()));
/// ```
pub(crate) fn extract_command_hint(raw_line: &[u8]) -> Option<String> {
    let mut pos = 0;

    // Skip tags: @tag=value tag2=value ... (terminated by space)
    if pos < raw_line.len() && raw_line[pos] == b'@' {
        while pos < raw_line.len() && raw_line[pos] != b' ' {
            pos += 1;
        }
        // Skip the space after tags
        if pos < raw_line.len() && raw_line[pos] == b' ' {
            pos += 1;
        }
    }

    // Skip prefix: :server or :nick!user@host (terminated by space)
    if pos < raw_line.len() && raw_line[pos] == b':' {
        while pos < raw_line.len() && raw_line[pos] != b' ' {
            pos += 1;
        }
        // Skip the space after prefix
        if pos < raw_line.len() && raw_line[pos] == b' ' {
            pos += 1;
        }
    }

    // Extract command: all alphanumeric characters until space or end
    let cmd_start = pos;
    while pos < raw_line.len() && (raw_line[pos].is_ascii_alphanumeric()) {
        pos += 1;
    }

    if pos > cmd_start {
        // Command found - decode as ASCII (safe because we know it's all alphanumeric)
        let cmd_bytes = &raw_line[cmd_start..pos];
        String::from_utf8(cmd_bytes.to_vec()).ok()
    } else {
        None
    }
}

/// Top-level protocol errors.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ProtocolError {
    /// I/O error during reading or writing.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// UTF-8 decoding error.
    #[error("decode error: {0}")]
    Decode(#[from] std::string::FromUtf8Error),

    /// Invalid UTF-8 bytes in message.
    ///
    /// Preserves the raw line (as bytes) and an optional command hint to allow
    /// proper error recovery and FAIL message generation in IRC daemons.
    #[error("invalid UTF-8 in message at byte {byte_pos}: {details}")]
    InvalidUtf8 {
        /// The raw message line as bytes (before UTF-8 validation failed).
        raw_line: Vec<u8>,
        /// Byte position where UTF-8 validation failed.
        byte_pos: usize,
        /// Detailed error message from UTF-8 decoder.
        details: String,
        /// Optional command name extracted from the raw line.
        /// Extracted by parsing the raw bytes as ASCII before UTF-8 validation.
        /// Format: first space-delimited token after tags (@...) and prefix (:...)
        command_hint: Option<String>,
    },

    /// Message exceeded maximum allowed length.
    #[error("message too long: {actual} bytes (limit: {limit})")]
    MessageTooLong {
        /// Actual message length.
        actual: usize,
        /// Maximum allowed length.
        limit: usize,
    },

    /// Tags section exceeded maximum allowed length.
    #[error("tags too long: {actual} bytes (limit: {limit})")]
    TagsTooLong {
        /// Actual tags length.
        actual: usize,
        /// Maximum allowed length.
        limit: usize,
    },

    /// Illegal control character in message.
    #[error("illegal control character: {0:?}")]
    IllegalControlChar(char),

    /// Failed to parse an IRC message.
    #[error("invalid message: {string}")]
    InvalidMessage {
        /// The invalid message string.
        string: String,
        /// The underlying parse error.
        #[source]
        cause: MessageParseError,
    },

    /// WebSocket transport not supported for this operation.
    #[error("WebSocket transport not supported")]
    WebSocketNotSupported,
}

/// Errors encountered when parsing IRC messages.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MessageParseError {
    /// Message was empty.
    #[error("empty message")]
    EmptyMessage,

    /// Command was invalid or missing.
    #[error("invalid command")]
    InvalidCommand,

    /// Not enough arguments for command.
    #[error("not enough arguments: expected {expected}, got {got}")]
    NotEnoughArguments {
        /// Expected number of arguments.
        expected: usize,
        /// Actual number of arguments.
        got: usize,
    },

    /// An argument was invalid.
    #[error("invalid argument: {0}")]
    InvalidArgument(String),

    /// Unknown command name.
    #[error("unknown command: {0}")]
    UnknownCommand(String),

    /// Invalid mode argument.
    #[error("invalid mode argument: {0}")]
    InvalidModeArg(String),

    /// Failed to parse mode string.
    #[error("invalid mode string: {string}")]
    InvalidModeString {
        /// The raw mode string.
        string: String,
        /// The underlying parse error.
        #[source]
        cause: ModeParseError,
    },

    /// Invalid subcommand for a command.
    #[error("invalid {cmd} subcommand: {sub}")]
    InvalidSubcommand {
        /// The parent command name.
        cmd: &'static str,
        /// The invalid subcommand.
        sub: String,
    },

    /// Invalid message prefix.
    #[error("invalid prefix: {0}")]
    InvalidPrefix(String),

    /// Invalid character at specific position.
    #[error("invalid character {byte:#04x} at position {position}")]
    InvalidCharacterAt {
        /// The invalid byte value.
        byte: u8,
        /// Position in the message.
        position: usize,
    },

    /// Tags section was not properly terminated.
    #[error("unterminated tags section")]
    UnterminatedTags,

    /// Origin/prefix was not properly terminated.
    #[error("unterminated origin/prefix")]
    UnterminatedOrigin,

    /// Parsing error with detailed context information.
    #[error("parsing failed at position {position}: {context}")]
    ParseContext {
        /// Character position where parsing failed.
        position: usize,
        /// Description of what was being parsed.
        context: String,
        /// Optional source error that caused this failure.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
        /// Preserved source error message for Clone support.
        /// When cloning, the boxed error cannot be cloned, but this field
        /// preserves the error message for debugging purposes.
        source_message: Option<String>,
    },
}

impl Clone for MessageParseError {
    fn clone(&self) -> Self {
        match self {
            MessageParseError::EmptyMessage => MessageParseError::EmptyMessage,
            MessageParseError::InvalidCommand => MessageParseError::InvalidCommand,
            MessageParseError::NotEnoughArguments { expected, got } => {
                MessageParseError::NotEnoughArguments {
                    expected: *expected,
                    got: *got,
                }
            }
            MessageParseError::InvalidArgument(s) => MessageParseError::InvalidArgument(s.clone()),
            MessageParseError::UnknownCommand(s) => MessageParseError::UnknownCommand(s.clone()),
            MessageParseError::InvalidModeArg(s) => MessageParseError::InvalidModeArg(s.clone()),
            MessageParseError::InvalidModeString { string, cause } => {
                MessageParseError::InvalidModeString {
                    string: string.clone(),
                    cause: cause.clone(),
                }
            }
            MessageParseError::InvalidSubcommand { cmd, sub } => {
                MessageParseError::InvalidSubcommand {
                    cmd,
                    sub: sub.clone(),
                }
            }
            MessageParseError::InvalidPrefix(s) => MessageParseError::InvalidPrefix(s.clone()),
            MessageParseError::InvalidCharacterAt { byte, position } => {
                MessageParseError::InvalidCharacterAt {
                    byte: *byte,
                    position: *position,
                }
            }
            MessageParseError::UnterminatedTags => MessageParseError::UnterminatedTags,
            MessageParseError::UnterminatedOrigin => MessageParseError::UnterminatedOrigin,
            MessageParseError::ParseContext {
                position,
                context,
                source,
                source_message,
            } => {
                // We can't clone the boxed error, but we preserve the error message
                let preserved_message = source_message
                    .clone()
                    .or_else(|| source.as_ref().map(|e| e.to_string()));
                MessageParseError::ParseContext {
                    position: *position,
                    context: context.clone(),
                    source: None,
                    source_message: preserved_message,
                }
            }
        }
    }
}

/// Errors encountered when parsing mode strings.
#[derive(Debug, Error, Clone)]
#[non_exhaustive]
pub enum ModeParseError {
    /// Invalid mode modifier character (not + or -).
    #[error("invalid mode modifier: {modifier}")]
    InvalidModeModifier {
        /// The invalid modifier character.
        modifier: char,
    },

    /// Missing mode modifier (+ or -).
    #[error("missing mode modifier")]
    MissingModeModifier,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ProtocolError::MessageTooLong {
            actual: 1024,
            limit: 512,
        };
        assert_eq!(
            format!("{}", err),
            "message too long: 1024 bytes (limit: 512)"
        );

        let err = MessageParseError::NotEnoughArguments {
            expected: 2,
            got: 1,
        };
        assert_eq!(
            format!("{}", err),
            "not enough arguments: expected 2, got 1"
        );
    }

    #[test]
    fn test_error_source_chaining() {
        // Test MessageParseError with source
        let mode_err = ModeParseError::MissingModeModifier;
        let parse_err = MessageParseError::InvalidModeString {
            string: "+xyz".to_string(),
            cause: mode_err.clone(),
        };

        // Test that the source is properly chained
        let source = std::error::Error::source(&parse_err);
        assert!(source.is_some());
        assert_eq!(source.unwrap().to_string(), mode_err.to_string());
    }

    #[test]
    fn test_protocol_error_chaining() {
        let parse_err = MessageParseError::InvalidCommand;
        let protocol_err = ProtocolError::InvalidMessage {
            string: "INVALID".to_string(),
            cause: parse_err.clone(),
        };

        // Test source chaining at protocol level
        let source = std::error::Error::source(&protocol_err);
        assert!(source.is_some());
        assert_eq!(source.unwrap().to_string(), parse_err.to_string());
    }

    #[test]
    fn test_parse_context_error() {
        let context_err = MessageParseError::ParseContext {
            position: 10,
            context: "parsing command arguments".to_string(),
            source: None,
            source_message: None,
        };

        assert_eq!(
            format!("{}", context_err),
            "parsing failed at position 10: parsing command arguments"
        );

        // Test with source error
        let io_err =
            std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "unexpected end of input");
        let context_err_with_source = MessageParseError::ParseContext {
            position: 5,
            context: "reading message data".to_string(),
            source: Some(Box::new(io_err)),
            source_message: None,
        };

        let source = std::error::Error::source(&context_err_with_source);
        assert!(source.is_some());

        // Test that cloning preserves the source error message
        let cloned = context_err_with_source.clone();
        match cloned {
            MessageParseError::ParseContext {
                source,
                source_message,
                ..
            } => {
                // After cloning, source is None but source_message is preserved
                assert!(source.is_none());
                assert!(source_message.is_some());
                assert_eq!(source_message.unwrap(), "unexpected end of input");
            }
            _ => panic!("Expected ParseContext variant"),
        }
    }

    #[test]
    fn test_error_conversion() {
        // Test automatic conversion from std::io::Error
        let io_err =
            std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "connection refused");
        let protocol_err: ProtocolError = io_err.into();

        match protocol_err {
            ProtocolError::Io(_) => {} // Expected
            _ => panic!("Expected Io variant"),
        }

        // Test conversion from FromUtf8Error
        let utf8_err = String::from_utf8(vec![0xff, 0xfe]).unwrap_err();
        let protocol_err: ProtocolError = utf8_err.into();

        match protocol_err {
            ProtocolError::Decode(_) => {} // Expected
            _ => panic!("Expected Decode variant"),
        }
    }
}
