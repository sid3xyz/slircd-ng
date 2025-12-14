//! Unified error handling for slircd-ng.
//!
//! This module provides a centralized error hierarchy for the IRC server,
//! with automatic conversions, IRC reply generation, and metric labeling.

use slirc_proto::{Command, Message, Prefix, Response};
use thiserror::Error;
use tokio::sync::mpsc;

// ============================================================================
// Handler Errors (command processing)
// ============================================================================

/// Errors that can occur during command handling.
#[derive(Debug, Error)]
#[allow(clippy::large_enum_variant)] // Send variant is large but rarely constructed
pub enum HandlerError {
    #[error("not enough parameters")]
    NeedMoreParams,

    #[error("no text to send")]
    NoTextToSend,

    #[error("nickname in use: {0}")]
    NicknameInUse(String),

    #[error("erroneous nickname: {0}")]
    ErroneousNickname(String),

    #[error("not registered")]
    NotRegistered,

    /// Disconnect the client silently (error message already sent)
    #[error("access denied")]
    AccessDenied,

    #[error("already registered")]
    AlreadyRegistered,

    #[error("internal error: nick or user missing after registration")]
    NickOrUserMissing,

    #[error("send error: {0}")]
    Send(#[from] mpsc::error::SendError<Message>),

    #[error("client quit: {0:?}")]
    Quit(Option<String>),

    #[error("internal error: {0}")]
    Internal(String),
}

impl HandlerError {
    /// Get a static error code string for metrics labeling.
    #[inline]
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::NeedMoreParams => "need_more_params",
            Self::NoTextToSend => "no_text_to_send",
            Self::NicknameInUse(_) => "nickname_in_use",
            Self::ErroneousNickname(_) => "erroneous_nickname",
            Self::NotRegistered => "not_registered",
            Self::AccessDenied => "access_denied",
            Self::AlreadyRegistered => "already_registered",
            Self::NickOrUserMissing => "nick_or_user_missing",
            Self::Send(_) => "send_error",
            Self::Quit(_) => "quit",
            Self::Internal(_) => "internal_error",
        }
    }

    /// Convert to an IRC error reply message.
    ///
    /// Returns `None` for errors that don't warrant a client-visible reply
    /// (e.g., internal errors, send failures, quit).
    pub fn to_irc_reply(&self, server_name: &str, nick: &str, cmd_name: &str) -> Option<Message> {
        match self {
            Self::NotRegistered => Some(Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name.to_string())),
                command: Command::Response(
                    Response::ERR_NOTREGISTERED,
                    vec!["*".to_string(), "You have not registered".to_string()],
                ),
            }),
            Self::NeedMoreParams => Some(Message {
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
            Self::NoTextToSend => Some(Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name.to_string())),
                command: Command::Response(
                    Response::ERR_NOTEXTTOSEND,
                    vec![nick.to_string(), "No text to send".to_string()],
                ),
            }),
            Self::NicknameInUse(bad_nick) => Some(Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name.to_string())),
                command: Command::Response(
                    Response::ERR_NICKNAMEINUSE,
                    vec![
                        nick.to_string(),
                        bad_nick.clone(),
                        "Nickname is already in use".to_string(),
                    ],
                ),
            }),
            Self::ErroneousNickname(bad_nick) => Some(Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name.to_string())),
                command: Command::Response(
                    Response::ERR_ERRONEOUSNICKNAME,
                    vec![
                        nick.to_string(),
                        bad_nick.clone(),
                        "Erroneous nickname".to_string(),
                    ],
                ),
            }),
            Self::AlreadyRegistered => Some(Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name.to_string())),
                command: Command::Response(
                    Response::ERR_ALREADYREGISTERED,
                    vec!["*".to_string(), "You may not reregister".to_string()],
                ),
            }),
            // These errors don't get client-visible replies
            Self::AccessDenied => None,          // Error already sent
            Self::NickOrUserMissing => None,     // Internal error
            Self::Send(_) => None,               // Internal error
            Self::Quit(_) => None,               // Handled specially by connection loop
            Self::Internal(_) => None,           // Internal error
        }
    }
}

/// Result type for command handlers.
pub type HandlerResult = Result<(), HandlerError>;

// ============================================================================
// Channel Errors (actor operations)
// ============================================================================

/// Channel operation errors.
///
/// These errors represent channel-specific failures that can be mapped
/// to RFC-compliant error responses by handler code.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ChannelError {
    #[error("not on channel")]
    NotOnChannel,

    #[error("you're not channel operator")]
    ChanOpPrivsNeeded,

    #[error("user {0} is not on that channel")]
    UserNotInChannel(String),

    #[error("user {0} is already on that channel")]
    UserOnChannel(String),

    #[error("cannot knock on this channel")]
    CannotKnock,

    #[error("channel is open")]
    ChanOpen,

    #[error("channel is restarting")]
    ChannelTombstone,

    #[error("session invalid")]
    SessionInvalid,

    #[error("cannot join channel (+b)")]
    BannedFromChan,

    #[error("cannot join channel (+i)")]
    InviteOnlyChan,

    #[error("cannot join channel (+l)")]
    ChannelIsFull,

    #[error("cannot join channel (+k)")]
    BadChannelKey,

    #[error("channel key already set")]
    #[allow(dead_code)]
    KeySet,

    #[error("{0} is unknown mode char to me for {1}")]
    #[allow(dead_code)]
    UnknownMode(char, String),

    #[error("channel doesn't support modes")]
    #[allow(dead_code)]
    NoChanModes,

    #[error("channel list {0} is full")]
    #[allow(dead_code)]
    BanListFull(char),

    #[error("you're not the original channel operator")]
    #[allow(dead_code)]
    UniqOpPrivsNeeded,

    #[error("{0}")]
    #[allow(dead_code)]
    UnknownError(String),
}

impl ChannelError {
    /// Get a static error code string for metrics labeling.
    #[inline]
    #[allow(dead_code)] // Available for future metrics integration
    pub fn error_code(&self) -> &'static str {
        match self {
            Self::NotOnChannel => "not_on_channel",
            Self::ChanOpPrivsNeeded => "chanop_privs_needed",
            Self::UserNotInChannel(_) => "user_not_in_channel",
            Self::UserOnChannel(_) => "user_on_channel",
            Self::CannotKnock => "cannot_knock",
            Self::ChanOpen => "chan_open",
            Self::ChannelTombstone => "channel_tombstone",
            Self::SessionInvalid => "session_invalid",
            Self::BannedFromChan => "banned_from_chan",
            Self::InviteOnlyChan => "invite_only_chan",
            Self::ChannelIsFull => "channel_is_full",
            Self::BadChannelKey => "bad_channel_key",
            Self::KeySet => "key_set",
            Self::UnknownMode(_, _) => "unknown_mode",
            Self::NoChanModes => "no_chan_modes",
            Self::BanListFull(_) => "ban_list_full",
            Self::UniqOpPrivsNeeded => "uniq_op_privs_needed",
            Self::UnknownError(_) => "unknown_error",
        }
    }

    /// Convert to an IRC error reply message.
    #[allow(dead_code)] // Available for handler use
    pub fn to_irc_reply(&self, server_name: &str, nick: &str, channel: &str) -> Message {
        let (response, args) = match self {
            Self::NotOnChannel => (
                Response::ERR_NOTONCHANNEL,
                vec![nick.to_string(), channel.to_string(), "You're not on that channel".to_string()],
            ),
            Self::ChanOpPrivsNeeded => (
                Response::ERR_CHANOPRIVSNEEDED,
                vec![nick.to_string(), channel.to_string(), "You're not channel operator".to_string()],
            ),
            Self::UserNotInChannel(target) => (
                Response::ERR_USERNOTINCHANNEL,
                vec![nick.to_string(), target.clone(), channel.to_string(), "They aren't on that channel".to_string()],
            ),
            Self::UserOnChannel(target) => (
                Response::ERR_USERONCHANNEL,
                vec![nick.to_string(), target.clone(), channel.to_string(), "is already on channel".to_string()],
            ),
            Self::BannedFromChan => (
                Response::ERR_BANNEDFROMCHAN,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+b)".to_string()],
            ),
            Self::InviteOnlyChan => (
                Response::ERR_INVITEONLYCHAN,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+i)".to_string()],
            ),
            Self::ChannelIsFull => (
                Response::ERR_CHANNELISFULL,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+l)".to_string()],
            ),
            Self::BadChannelKey => (
                Response::ERR_BADCHANNELKEY,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+k)".to_string()],
            ),
            Self::UnknownMode(c, chan) => (
                Response::ERR_UNKNOWNMODE,
                vec![nick.to_string(), c.to_string(), format!("is unknown mode char to me for {}", chan)],
            ),
            Self::BanListFull(list_char) => (
                Response::ERR_BANLISTFULL,
                vec![nick.to_string(), channel.to_string(), list_char.to_string(), "Channel list is full".to_string()],
            ),
            Self::UniqOpPrivsNeeded => (
                Response::ERR_UNIQOPPRIVSNEEDED,
                vec![nick.to_string(), "You're not the original channel operator".to_string()],
            ),
            // These don't have standard IRC numerics - use generic error
            Self::CannotKnock | Self::ChanOpen | Self::ChannelTombstone
            | Self::SessionInvalid | Self::KeySet | Self::NoChanModes
            | Self::UnknownError(_) => (
                Response::ERR_UNKNOWNERROR,
                vec![nick.to_string(), channel.to_string(), self.to_string()],
            ),
        };

        Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.to_string())),
            command: Command::Response(response, args),
        }
    }
}

// ============================================================================
// Database Errors (re-exported, kept in db module for sqlx proximity)
// ============================================================================

// DbError stays in db/mod.rs because it has #[from] sqlx::Error which requires
// sqlx to be in scope. We just document that it exists there.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handler_error_codes() {
        assert_eq!(HandlerError::NeedMoreParams.error_code(), "need_more_params");
        assert_eq!(HandlerError::NotRegistered.error_code(), "not_registered");
        assert_eq!(HandlerError::Internal("test".into()).error_code(), "internal_error");
    }

    #[test]
    fn test_channel_error_codes() {
        assert_eq!(ChannelError::NotOnChannel.error_code(), "not_on_channel");
        assert_eq!(ChannelError::BannedFromChan.error_code(), "banned_from_chan");
    }

    #[test]
    fn test_handler_error_to_irc_reply() {
        let reply = HandlerError::NeedMoreParams.to_irc_reply("server", "nick", "JOIN");
        assert!(reply.is_some());

        // Internal errors don't generate replies
        let reply = HandlerError::Internal("oops".into()).to_irc_reply("server", "nick", "JOIN");
        assert!(reply.is_none());
    }

    #[test]
    fn test_channel_error_to_irc_reply() {
        let reply = ChannelError::NotOnChannel.to_irc_reply("server", "nick", "#test");
        assert!(matches!(reply.command, Command::Response(Response::ERR_NOTONCHANNEL, _)));
    }
}
