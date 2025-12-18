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

    #[error("no such channel: {0}")]
    NoSuchChannel(String),

    #[error("unknown command: {0}")]
    UnknownCommand(String),

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
            Self::NoSuchChannel(_) => "no_such_channel",
            Self::UnknownCommand(_) => "unknown_command",
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
        let mut msg = match self {
            Self::NotRegistered => Response::err_notregistered(nick),
            Self::NeedMoreParams => Response::err_needmoreparams(nick, cmd_name),
            Self::NoTextToSend => Response::err_notexttosend(nick),
            Self::NicknameInUse(bad_nick) => Response::err_nicknameinuse(nick, bad_nick),
            Self::ErroneousNickname(bad_nick) => Response::err_erroneusnickname(nick, bad_nick),
            Self::AlreadyRegistered => Response::err_alreadyregistred(nick),
            Self::NoSuchChannel(bad_chan) => Response::err_nosuchchannel(nick, bad_chan),
            Self::UnknownCommand(cmd) => Response::err_unknowncommand(nick, cmd),

            // These errors don't get client-visible replies
            Self::AccessDenied => return None,
            Self::NickOrUserMissing => return None,
            Self::Send(_) => return None,
            Self::Quit(_) => return None,
            Self::Internal(_) => return None,
        };

        // Set the prefix to the server name
        msg.prefix = Some(Prefix::ServerName(server_name.to_string()));
        Some(msg)
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

    #[error("cannot join channel (+R)")]
    NeedReggedNick,

    #[error("cannot join channel (+z)")]
    SecureOnlyChan,

    #[error("cannot join channel (+O)")]
    OperOnlyChan,

    #[error("cannot join channel (+A)")]
    AdminOnlyChan,

    #[error("kicks are disabled in this channel (+Q)")]
    NoKicksActive,

    #[error("invites are disabled in this channel (+V)")]
    NoInviteActive,
}

impl ChannelError {
    /// Convert to an IRC error reply message.
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
            Self::NeedReggedNick => (
                // 477 is commonly used for "you need to register to join"
                Response::ERR_NOCHANMODES,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+R) - you need to be identified with services".to_string()],
            ),
            Self::SecureOnlyChan => (
                Response::ERR_SECUREONLYCHAN,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+z) - you need to be connected via TLS".to_string()],
            ),
            Self::OperOnlyChan => (
                Response::ERR_OPERONLY,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+O) - you need to be an IRC operator".to_string()],
            ),
            // AdminOnlyChan uses ERR_OPERONLY as well since there's no standard admin-only numeric
            Self::AdminOnlyChan => (
                Response::ERR_OPERONLY,
                vec![nick.to_string(), channel.to_string(), "Cannot join channel (+A) - you need to be a server administrator".to_string()],
            ),
            Self::NoKicksActive => (
                Response::ERR_UNKNOWNERROR,
                vec![nick.to_string(), channel.to_string(), "Kicks are disabled in this channel (+Q)".to_string()],
            ),
            Self::NoInviteActive => (
                Response::ERR_UNKNOWNERROR,
                vec![nick.to_string(), channel.to_string(), "Invites are disabled in this channel (+V)".to_string()],
            ),
            // These don't have standard IRC numerics - use generic error
            Self::CannotKnock | Self::ChanOpen | Self::ChannelTombstone
            | Self::SessionInvalid => (
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
