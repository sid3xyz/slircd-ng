//! Helper functions for IRC command handlers.
//!
//! This module contains common error reply builders, labeled-response helpers,
//! and hostmask matching used across multiple handlers.
//!
//! Note: User lookup helpers (`resolve_nick_to_uid`, `get_nick_or_star`, etc.)
//! remain in `mod.rs` because they depend on `Context` which is defined there.

use slirc_proto::{Command, Message, Prefix, Response, Tag};

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

/// Create ERR_NOPRIVILEGES reply (481) - user is not an IRC operator.
pub fn err_noprivileges(server_name: &str, nick: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOPRIVILEGES,
        vec![
            nick.to_string(),
            "Permission Denied - You're not an IRC operator".to_string(),
        ],
    )
}

/// Create ERR_NEEDMOREPARAMS reply (461) - not enough parameters.
pub fn err_needmoreparams(server_name: &str, nick: &str, command: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NEEDMOREPARAMS,
        vec![
            nick.to_string(),
            command.to_string(),
            "Not enough parameters".to_string(),
        ],
    )
}

/// Create ERR_NOSUCHNICK reply (401) - no such nick/channel.
pub fn err_nosuchnick(server_name: &str, nick: &str, target: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOSUCHNICK,
        vec![
            nick.to_string(),
            target.to_string(),
            "No such nick/channel".to_string(),
        ],
    )
}

/// Create ERR_NOSUCHCHANNEL reply (403) - no such channel.
pub fn err_nosuchchannel(server_name: &str, nick: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOSUCHCHANNEL,
        vec![
            nick.to_string(),
            channel.to_string(),
            "No such channel".to_string(),
        ],
    )
}

/// Create ERR_NOTONCHANNEL reply (442) - you're not on that channel.
pub fn err_notonchannel(server_name: &str, nick: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOTONCHANNEL,
        vec![
            nick.to_string(),
            channel.to_string(),
            "You're not on that channel".to_string(),
        ],
    )
}

/// Create ERR_CHANOPRIVSNEEDED reply (482) - you're not channel operator.
pub fn err_chanoprivsneeded(server_name: &str, nick: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_CHANOPRIVSNEEDED,
        vec![
            nick.to_string(),
            channel.to_string(),
            "You're not channel operator".to_string(),
        ],
    )
}

/// Create ERR_USERNOTINCHANNEL reply (441) - they aren't on that channel.
pub fn err_usernotinchannel(server_name: &str, nick: &str, target: &str, channel: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_USERNOTINCHANNEL,
        vec![
            nick.to_string(),
            target.to_string(),
            channel.to_string(),
            "They aren't on that channel".to_string(),
        ],
    )
}

/// Create ERR_NOTREGISTERED reply (451) - you have not registered.
pub fn err_notregistered(server_name: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_NOTREGISTERED,
        vec!["*".to_string(), "You have not registered".to_string()],
    )
}

/// Create ERR_UNKNOWNCOMMAND reply (421) - unknown command.
pub fn err_unknowncommand(server_name: &str, nick: &str, command: &str) -> Message {
    server_reply(
        server_name,
        Response::ERR_UNKNOWNCOMMAND,
        vec![
            nick.to_string(),
            command.to_string(),
            "Unknown command".to_string(),
        ],
    )
}

// ============================================================================
// Labeled Response Helpers (IRCv3)
// ============================================================================

/// Attach a label tag to a message if one was provided.
///
/// Used for IRCv3 labeled-response capability to echo the client's label.
#[allow(dead_code)] // Will be used in Phase 4 for full labeled-response support
pub fn with_label(msg: Message, label: Option<&str>) -> Message {
    match label {
        Some(value) => msg.with_tag("label", Some(value)),
        None => msg,
    }
}

/// Create a labeled ACK response for commands that normally produce no output.
///
/// Per IRCv3 labeled-response spec, servers MUST respond with ACK when a labeled
/// command would normally produce no response (e.g., PONG).
pub fn labeled_ack(server_name: &str, label: &str) -> Message {
    Message {
        tags: Some(vec![Tag(std::borrow::Cow::Borrowed("label"), Some(label.to_string()))]),
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
// Hostmask matching (shared by channel, messaging, oper handlers)
// ============================================================================

/// Check if a hostmask (nick!user@host) matches a pattern.
/// Supports wildcards (* and ?).
///
/// # Examples
/// ```ignore
/// matches_hostmask("*!*@*.example.com", "nick!user@host.example.com") // true
/// matches_hostmask("nick!*@*", "nick!user@host") // true
/// matches_hostmask("*!user@*", "nick!other@host") // false
/// ```
pub fn matches_hostmask(pattern: &str, hostmask: &str) -> bool {
    let pattern = pattern.to_lowercase();
    let hostmask = hostmask.to_lowercase();

    let mut p_chars = pattern.chars().peekable();
    let mut h_chars = hostmask.chars().peekable();

    while let Some(p) = p_chars.next() {
        match p {
            '*' => {
                // Consume consecutive *
                while p_chars.peek() == Some(&'*') {
                    p_chars.next();
                }
                // If * is at end, match rest
                if p_chars.peek().is_none() {
                    return true;
                }
                // Try matching from each position
                while h_chars.peek().is_some() {
                    let remaining_pattern: String = p_chars.clone().collect();
                    let remaining_hostmask: String = h_chars.clone().collect();
                    if matches_hostmask(&remaining_pattern, &remaining_hostmask) {
                        return true;
                    }
                    h_chars.next();
                }
                return matches_hostmask(&p_chars.collect::<String>(), "");
            }
            '?' => {
                if h_chars.next().is_none() {
                    return false;
                }
            }
            c => {
                if h_chars.next() != Some(c) {
                    return false;
                }
            }
        }
    }

    h_chars.peek().is_none()
}
