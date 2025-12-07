//! Helper functions for IRC command handlers.
//!
//! This module contains common error reply builders and labeled-response helpers.
//!
//! Note: User lookup helpers (`resolve_nick_to_uid`, `get_nick_or_star`, etc.)
//! remain in `mod.rs` because they depend on `Context` which is defined there.

use slirc_proto::{Command, Message, Prefix, Response, Tag};

// Re-export hostmask matching from proto for use by handlers
pub use slirc_proto::matches_hostmask;

// ============================================================================
// Common reply helpers
// ============================================================================

/// Helper to create a server reply message (numeric response).
pub fn server_reply(server_name: &str, response: Response, params: Vec<String>) -> Message {
    Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::Response(response, params),
    }
}

/// Helper to create a server NOTICE message.
///
/// Uses slirc-proto's `Message::notice()` constructor with server prefix.
pub fn server_notice<T: Into<String>>(server_name: &str, target: &str, text: T) -> Message {
    Message::notice(target, text).with_prefix(Prefix::ServerName(server_name.to_string()))
}

// ============================================================================
// Error reply helpers (macro-generated)
// ============================================================================

/// Macro to generate error reply helper functions.
///
/// This macro eliminates boilerplate by generating functions that follow the pattern:
/// `server_reply(server_name, Response::ERR_X, vec![param1, param2, ..., message])`
///
/// ## Syntax:
///
/// ```ignore
/// error_reply! {
///     /// Doc comment
///     fn_name(response_code, "error message") => {
///         server_name: &str,
///         param1: &str,
///         param2: &str,
///         ...
///     }
/// }
/// ```
///
/// ## Examples:
///
/// - Simple error (nick + message):
///   `err_noprivileges(ERR_NOPRIVILEGES, "...") => { server_name, nick }`
///
/// - Error with extra param (nick + param + message):
///   `err_needmoreparams(ERR_NEEDMOREPARAMS, "...") => { server_name, nick, command }`
///
/// - Error without nick (uses "*"):
///   `err_notregistered(ERR_NOTREGISTERED, "...") => { server_name }`
///
/// ## Limitations:
///
/// - Parameters are always converted with `.to_string()`
/// - Error message is always the last parameter
/// - Function signatures are fixed; cannot add custom logic
/// - No support for optional parameters or default values
macro_rules! error_reply {
    // Pattern: No additional params (just server_name) — uses "*" as nick
    (
        $(#[$meta:meta])*
        $fn_name:ident($response:ident, $message:expr) => {
            server_name: &str
        }
    ) => {
        $(#[$meta])*
        pub fn $fn_name(server_name: &str) -> Message {
            server_reply(
                server_name,
                Response::$response,
                vec!["*".to_string(), $message.to_string()],
            )
        }
    };

    // Pattern: Nick only (server_name, nick)
    (
        $(#[$meta:meta])*
        $fn_name:ident($response:ident, $message:expr) => {
            server_name: &str,
            nick: &str
        }
    ) => {
        $(#[$meta])*
        pub fn $fn_name(server_name: &str, nick: &str) -> Message {
            server_reply(
                server_name,
                Response::$response,
                vec![nick.to_string(), $message.to_string()],
            )
        }
    };

    // Pattern: Nick + 1 param (server_name, nick, param1)
    (
        $(#[$meta:meta])*
        $fn_name:ident($response:ident, $message:expr) => {
            server_name: &str,
            nick: &str,
            $param1:ident: &str
        }
    ) => {
        $(#[$meta])*
        pub fn $fn_name(server_name: &str, nick: &str, $param1: &str) -> Message {
            server_reply(
                server_name,
                Response::$response,
                vec![nick.to_string(), $param1.to_string(), $message.to_string()],
            )
        }
    };

    // Pattern: Nick + 2 params (server_name, nick, param1, param2)
    (
        $(#[$meta:meta])*
        $fn_name:ident($response:ident, $message:expr) => {
            server_name: &str,
            nick: &str,
            $param1:ident: &str,
            $param2:ident: &str
        }
    ) => {
        $(#[$meta])*
        pub fn $fn_name(server_name: &str, nick: &str, $param1: &str, $param2: &str) -> Message {
            server_reply(
                server_name,
                Response::$response,
                vec![
                    nick.to_string(),
                    $param1.to_string(),
                    $param2.to_string(),
                    $message.to_string(),
                ],
            )
        }
    };
}

// Generate all error reply functions using the macro
error_reply! {
    /// Create ERR_NOPRIVILEGES reply (481) - user is not an IRC operator.
    err_noprivileges(ERR_NOPRIVILEGES, "Permission Denied - You're not an IRC operator") => {
        server_name: &str,
        nick: &str
    }
}

error_reply! {
    /// Create ERR_NEEDMOREPARAMS reply (461) - not enough parameters.
    err_needmoreparams(ERR_NEEDMOREPARAMS, "Not enough parameters") => {
        server_name: &str,
        nick: &str,
        command: &str
    }
}

error_reply! {
    /// Create ERR_NOSUCHNICK reply (401) - no such nick/channel.
    err_nosuchnick(ERR_NOSUCHNICK, "No such nick/channel") => {
        server_name: &str,
        nick: &str,
        target: &str
    }
}

error_reply! {
    /// Create ERR_NOSUCHCHANNEL reply (403) - no such channel.
    err_nosuchchannel(ERR_NOSUCHCHANNEL, "No such channel") => {
        server_name: &str,
        nick: &str,
        channel: &str
    }
}

error_reply! {
    /// Create ERR_NOTONCHANNEL reply (442) - you're not on that channel.
    err_notonchannel(ERR_NOTONCHANNEL, "You're not on that channel") => {
        server_name: &str,
        nick: &str,
        channel: &str
    }
}

error_reply! {
    /// Create ERR_CHANOPRIVSNEEDED reply (482) - you're not channel operator.
    err_chanoprivsneeded(ERR_CHANOPRIVSNEEDED, "You're not channel operator") => {
        server_name: &str,
        nick: &str,
        channel: &str
    }
}

error_reply! {
    /// Create ERR_USERNOTINCHANNEL reply (441) - they aren't on that channel.
    err_usernotinchannel(ERR_USERNOTINCHANNEL, "They aren't on that channel") => {
        server_name: &str,
        nick: &str,
        target: &str,
        channel: &str
    }
}

error_reply! {
    /// Create ERR_NOTREGISTERED reply (451) - you have not registered.
    /// NOTE: This helper is no longer used since registration checks are
    /// handled centrally by the registry typestate dispatch (Innovation 1).
    /// Kept for potential future use.
    #[allow(dead_code)]
    err_notregistered(ERR_NOTREGISTERED, "You have not registered") => {
        server_name: &str
    }
}

error_reply! {
    /// Create ERR_UNKNOWNCOMMAND reply (421) - unknown command.
    err_unknowncommand(ERR_UNKNOWNCOMMAND, "Unknown command") => {
        server_name: &str,
        nick: &str,
        command: &str
    }
}

// ============================================================================
// Labeled Response Helpers (IRCv3)
// ============================================================================

/// Attach a label tag to a message if one was provided.
///
/// Middleware handles labeling automatically; handlers should only call this when constructing
/// messages outside the normal dispatch path.
pub fn with_label(msg: Message, label: Option<&str>) -> Message {
    match label {
        Some(value) => msg.with_tag("label", Some(value)),
        None => msg,
    }
}

/// Create a labeled ACK response for commands that normally produce no output.
///
/// Middleware now issues ACKs for labeled-response; handlers should rarely need this directly.
/// Per IRCv3 labeled-response spec, servers MUST respond with ACK when a labeled
/// command would normally produce no response (e.g., PONG).
pub fn labeled_ack(server_name: &str, label: &str) -> Message {
    Message {
        tags: Some(vec![Tag(
            std::borrow::Cow::Borrowed("label"),
            Some(label.to_string()),
        )]),
        prefix: Some(Prefix::ServerName(server_name.to_string())),
        command: Command::ACK,
    }
}

/// Helper to create a user prefix (nick!user@host).
///
/// Note: This is equivalent to `Prefix::new(nick, user, host)` from slirc-proto.
/// Consider using `Prefix::new()` directly for new code.
#[inline]
pub fn user_prefix(nick: &str, user: &str, host: &str) -> Prefix {
    Prefix::new(nick, user, host)
}

// ============================================================================
// Ban matching (extended bans + hostmask via proto)
// ============================================================================
