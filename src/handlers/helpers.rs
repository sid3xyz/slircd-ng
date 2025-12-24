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
// Argument extraction macro
// ============================================================================

/// Extract a required argument from a message, returning `NeedMoreParams` if missing.
///
/// # Usage
/// ```ignore
/// let target = require_arg!(msg, 0);  // Gets arg(0), returns NeedMoreParams on missing/empty
/// let text = require_arg!(msg, 1);    // Gets arg(1)
/// ```
#[macro_export]
macro_rules! require_arg {
    ($msg:expr, $idx:expr) => {
        match $msg.arg($idx) {
            Some(s) if !s.is_empty() => s,
            _ => return Err($crate::error::HandlerError::NeedMoreParams),
        }
    };
}

/// Extract a required argument from a message with full error handling.
///
/// Sends ERR_NEEDMOREPARAMS to the client, records metrics, and returns Ok(None)
/// if the argument is missing. Returns Ok(Some(arg)) on success.
///
/// # Usage
/// ```ignore
/// let Some(target) = require_arg_or_reply!(ctx, msg, 0, "PRIVMSG") else { return Ok(()); };
/// ```
#[macro_export]
macro_rules! require_arg_or_reply {
    ($ctx:expr, $msg:expr, $idx:expr, $cmd:expr) => {{
        match $msg.arg($idx) {
            Some(s) if !s.is_empty() => Some(s),
            _ => {
                let reply = slirc_proto::Response::err_needmoreparams($ctx.nick(), $cmd)
                    .with_prefix($ctx.server_prefix());
                let _ = $ctx.sender.send(reply).await;
                $crate::metrics::record_command_error($cmd, "ERR_NEEDMOREPARAMS");
                None
            }
        }
    }};
}

/// Send ERR_NOPRIVILEGES and record metrics, returning from handler.
///
/// Use this after a failed capability check for operator commands.
///
/// # Usage
/// ```ignore
/// if authority.request_kill_cap(ctx.uid).await.is_none() {
///     send_noprivileges!(ctx, "KILL");
///     return Ok(());
/// }
/// ```
#[macro_export]
macro_rules! send_noprivileges {
    ($ctx:expr, $cmd:expr) => {{
        let reply =
            slirc_proto::Response::err_noprivileges($ctx.nick()).with_prefix($ctx.server_prefix());
        let _ = $ctx.sender.send(reply).await;
        $crate::metrics::record_command_error($cmd, "ERR_NOPRIVILEGES");
    }};
}

/// Require admin capability for SA* commands.
///
/// Returns `Some(Cap)` if authorized, or sends ERR_NOPRIVILEGES and returns `None`.
///
/// # Usage
/// ```ignore
/// let Some(_cap) = require_admin_cap!(ctx, "SAJOIN") else { return Ok(()); };
/// ```
#[macro_export]
macro_rules! require_admin_cap {
    ($ctx:expr, $cmd:expr) => {{
        let authority = $ctx.authority();
        match authority.request_admin_cap($ctx.uid).await {
            Some(cap) => Some(cap),
            None => {
                $crate::send_noprivileges!($ctx, $cmd);
                None
            }
        }
    }};
}

/// Require an arbitrary oper capability.
///
/// Returns `Some(Cap)` if authorized, or sends ERR_NOPRIVILEGES and returns `None`.
///
/// # Usage
/// ```ignore
/// let Some(_cap) = require_oper_cap!(ctx, "KILL", request_kill_cap) else { return Ok(()); };
/// ```
#[macro_export]
macro_rules! require_oper_cap {
    ($ctx:expr, $cmd:expr, $cap_method:ident) => {{
        let authority = $ctx.authority();
        match authority.$cap_method($ctx.uid).await {
            Some(cap) => Some(cap),
            None => {
                $crate::send_noprivileges!($ctx, $cmd);
                None
            }
        }
    }};
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_reply_creates_correct_structure() {
        let response = Response::RPL_WELCOME;
        let params = vec!["nick".to_string(), "Welcome!".to_string()];
        let msg = server_reply("irc.example.net", response, params.clone());

        assert_eq!(
            msg.prefix,
            Some(Prefix::ServerName("irc.example.net".to_string()))
        );
        assert!(matches!(msg.command, Command::Response(Response::RPL_WELCOME, ref p) if *p == params));
        assert!(msg.tags.is_none());
    }

    #[test]
    fn test_server_reply_with_empty_params() {
        let msg = server_reply("srv", Response::RPL_LUSERCLIENT, vec![]);
        assert!(matches!(msg.command, Command::Response(Response::RPL_LUSERCLIENT, ref p) if p.is_empty()));
    }

    #[test]
    fn test_server_notice_creates_notice_with_prefix() {
        let msg = server_notice("irc.local", "#channel", "Hello, world!");

        assert_eq!(
            msg.prefix,
            Some(Prefix::ServerName("irc.local".to_string()))
        );
        assert!(matches!(&msg.command, Command::NOTICE(target, message)
            if target == "#channel" && message == "Hello, world!"));
    }

    #[test]
    fn test_server_notice_with_string_conversion() {
        // Test that Into<String> works with String input
        let text = String::from("Test message");
        let msg = server_notice("srv", "nick", text);
        assert!(matches!(&msg.command, Command::NOTICE(_, message) if message == "Test message"));
    }

    #[test]
    fn test_with_label_adds_label_when_some() {
        let original = Message::pong("irc.example.net");
        let labeled = with_label(original, Some("abc123"));

        // Check the label tag was added
        let tags = labeled.tags.expect("Tags should be Some");
        assert!(tags.iter().any(|t| t.0 == "label" && t.1.as_deref() == Some("abc123")));
    }

    #[test]
    fn test_with_label_returns_unchanged_when_none() {
        let original = Message::pong("irc.example.net");
        let original_clone = original.clone();
        let result = with_label(original, None);

        // Should be unchanged
        assert_eq!(result.prefix, original_clone.prefix);
        assert!(matches!(&result.command, Command::PONG { .. }));
    }

    #[test]
    fn test_labeled_ack_has_correct_structure() {
        let msg = labeled_ack("irc.example.net", "xyz789");

        // Check prefix
        assert_eq!(
            msg.prefix,
            Some(Prefix::ServerName("irc.example.net".to_string()))
        );

        // Check command is ACK
        assert!(matches!(msg.command, Command::ACK));

        // Check label tag
        let tags = msg.tags.expect("Tags should be Some");
        assert!(tags.iter().any(|t| t.0 == "label" && t.1.as_deref() == Some("xyz789")));
    }

    #[test]
    fn test_labeled_ack_with_empty_label() {
        let msg = labeled_ack("srv", "");

        // Empty label is valid (though unusual)
        let tags = msg.tags.expect("Tags should be Some");
        assert!(tags.iter().any(|t| t.0 == "label" && t.1.as_deref() == Some("")));
    }

    #[test]
    fn test_user_prefix_creates_correct_prefix() {
        let prefix = user_prefix("nick", "user", "host.example.com");

        assert!(matches!(prefix, Prefix::Nickname(ref n, ref u, ref h)
            if n == "nick" && u == "user" && h == "host.example.com"));
    }

    #[test]
    fn test_user_prefix_with_special_characters() {
        let prefix = user_prefix("nick[away]", "~user", "192.168.1.1");

        assert!(matches!(prefix, Prefix::Nickname(ref n, ref u, ref h)
            if n == "nick[away]" && u == "~user" && h == "192.168.1.1"));
    }
}
