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
                ProtocolError::InvalidUtf8(details) => ReadErrorAction::FatalProtocolError {
                    error_msg: format!("Invalid UTF-8 in message: {details}"),
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
