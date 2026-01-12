//! Error handling utilities for IRC connection management.
//!
//! Provides classification and conversion of transport and handler errors
//! into appropriate IRC protocol responses.

use slirc_proto::Message;
use slirc_proto::error::ProtocolError;
use slirc_proto::transport::TransportReadError;

use crate::error::HandlerError;

/// Classification of transport read errors for appropriate handling.
pub(super) enum ReadErrorAction {
    /// Recoverable line-too-long error - send ERR_INPUTTOOLONG (417) and continue
    InputTooLong,
    /// Recoverable invalid UTF-8 error - send FAIL <command> INVALID_UTF8 and continue
    InvalidUtf8 {
        command_hint: Option<String>,
        raw_line: Vec<u8>,
        details: String,
    },
    /// Fatal protocol violation - send ERROR message and disconnect
    FatalProtocolError { error_msg: String },
    /// I/O error - connection is broken, just log and disconnect
    IoError,
}

/// Classify a transport read error into an actionable category.
pub(super) fn classify_read_error(e: &TransportReadError) -> ReadErrorAction {
    match e {
        TransportReadError::Protocol(proto_err) => {
            match proto_err {
                // Recoverable: line or tags too long → ERR_INPUTTOOLONG (417)
                // Per Ergo/modern IRC: send 417 and continue, don't disconnect
                ProtocolError::MessageTooLong { .. } | ProtocolError::TagsTooLong { .. } => {
                    ReadErrorAction::InputTooLong
                }
                // Fatal: other protocol errors → ERROR and disconnect
                ProtocolError::IllegalControlChar(ch) => ReadErrorAction::FatalProtocolError {
                    error_msg: format!("Illegal control character: {ch:?}"),
                },
                ProtocolError::InvalidMessage { string, cause } => {
                    ReadErrorAction::FatalProtocolError {
                        error_msg: format!("Malformed message: {cause} (input: {string:?})"),
                    }
                }
                ProtocolError::InvalidUtf8 {
                    command_hint,
                    raw_line,
                    details,
                    ..
                } => ReadErrorAction::InvalidUtf8 {
                    command_hint: command_hint.clone(),
                    raw_line: raw_line.clone(),
                    details: details.clone(),
                },
                // Handle other variants that might be added in the future
                _ => ReadErrorAction::FatalProtocolError {
                    error_msg: format!("Protocol error: {proto_err}"),
                },
            }
        }
        TransportReadError::Io(_) => ReadErrorAction::IoError,
        // Handle future variants gracefully
        _ => ReadErrorAction::IoError,
    }
}

/// Extract label tag from raw message bytes (ASCII safe).
/// Returns None if no label tag found or if parsing fails.
pub(super) fn extract_label_from_raw(raw_line: &[u8]) -> Option<String> {
    // Tags start with @ and end at first space
    if raw_line.first()? != &b'@' {
        return None;
    }

    // Find the end of tags section (first space)
    let tags_end = raw_line.iter().position(|&b| b == b' ')?;
    let tags_section = &raw_line[1..tags_end]; // Skip '@'

    // Split by semicolon to get individual tags
    let mut start = 0;
    for (idx, &byte) in tags_section.iter().enumerate() {
        if byte == b';' || idx == tags_section.len() - 1 {
            let end = if idx == tags_section.len() - 1 {
                idx + 1
            } else {
                idx
            };
            let tag = &tags_section[start..end];

            // Check if this is a label tag
            if let Some(eq_pos) = tag.iter().position(|&b| b == b'=') {
                let key = &tag[..eq_pos];
                if key == b"label" {
                    let value = &tag[eq_pos + 1..];
                    // Convert to string, lossy since labels should be ASCII
                    return Some(String::from_utf8_lossy(value).into_owned());
                }
            }

            start = idx + 1;
        }
    }

    None
}

/// Convert a HandlerError to an appropriate IRC error reply using an owned Message.
///
/// This variant accepts an owned Message instead of MessageRef, for use when
/// the MessageRef has already been dropped (e.g., after transport operations).
pub(super) fn handler_error_to_reply_owned(
    server_name: &str,
    nick: &str,
    error: &HandlerError,
    msg: &Message,
) -> Option<Message> {
    let cmd_name = msg.command.name();
    error.to_irc_reply(server_name, nick, cmd_name)
}
