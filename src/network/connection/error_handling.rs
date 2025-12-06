//! Error handling utilities for IRC connection management.
//!
//! Provides classification and conversion of transport and handler errors
//! into appropriate IRC protocol responses.

use slirc_proto::error::ProtocolError;
use slirc_proto::transport::TransportReadError;
use slirc_proto::{Command, Message, Prefix, Response};

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

/// Convert a HandlerError to an appropriate IRC error reply.
///
/// Returns None for errors that don't warrant a client-visible reply
/// (e.g., internal errors, send failures).
pub(super) fn handler_error_to_reply(
    server_name: &str,
    nick: &str,
    error: &crate::handlers::HandlerError,
    msg: &slirc_proto::MessageRef<'_>,
) -> Option<Message> {
    use crate::handlers::HandlerError;

    let cmd_name = msg.command_name();

    match error {
        HandlerError::NotRegistered => Some(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::Response(
                Response::ERR_NOTREGISTERED,
                vec!["*".to_string(), "You have not registered".to_string()],
            ),
        }),
        HandlerError::NeedMoreParams => Some(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::Response(
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    nick.to_string(),
                    cmd_name.to_string(),
                    "Not enough parameters".to_string(),
                ],
            ),
        }),
        HandlerError::NoTextToSend => Some(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::Response(
                Response::ERR_NOTEXTTOSEND,
                vec![nick.to_string(), "No text to send".to_string()],
            ),
        }),
        HandlerError::NicknameInUse(nick) => Some(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::Response(
                Response::ERR_NICKNAMEINUSE,
                vec![
                    "*".to_string(),
                    nick.clone(),
                    "Nickname is already in use".to_string(),
                ],
            ),
        }),
        HandlerError::ErroneousNickname(nick) => Some(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::Response(
                Response::ERR_ERRONEOUSNICKNAME,
                vec![
                    "*".to_string(),
                    nick.clone(),
                    "Erroneous nickname".to_string(),
                ],
            ),
        }),
        HandlerError::AlreadyRegistered => Some(Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::Response(
                Response::ERR_ALREADYREGISTERED,
                vec!["*".to_string(), "You may not reregister".to_string()],
            ),
        }),
        // Access denied - error already sent to client, don't add another message
        HandlerError::AccessDenied => None,
        // Internal errors - don't expose to client
        HandlerError::NickOrUserMissing | HandlerError::Send(_) => None,
        // Quit is handled specially by the connection loop, not as an error reply
        HandlerError::Quit(_) => None,
        HandlerError::Internal(_) => None,
    }
}
