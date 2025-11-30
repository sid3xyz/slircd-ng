//! Messaging handlers.
//!
//! Handles PRIVMSG, NOTICE, and TAGMSG commands for both users and channels.
//! Uses a unified routing system to enforce channel modes (+n, +m).
//! Includes CTCP (Client-to-Client Protocol) handling for VERSION, PING, etc.

use super::{Context, Handler, HandlerError, HandlerResult, matches_hostmask, server_reply, user_prefix};
use crate::services::chanserv::route_chanserv_message;
use crate::services::nickserv::route_service_message;
use async_trait::async_trait;
use chrono::Local;
use slirc_proto::ctcp::{Ctcp, CtcpKind};
use slirc_proto::{Command, Message, MessageRef, Response, Tag, irc_to_lower};
use std::borrow::Cow;
use tracing::debug;

// ============================================================================
// Shun Checking
// ============================================================================

/// Check if a user is shunned (silently blocked from messaging).
///
/// Returns true if the user is shunned and their command should be silently ignored.
async fn is_shunned(ctx: &Context<'_>) -> bool {
    // Get user's hostmask
    let user_host = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        format!("{}@{}", user.user, user.host)
    } else {
        return false;
    };

    // Check database for shuns
    match ctx.db.bans().matches_shun(&user_host).await {
        Ok(Some(shun)) => {
            debug!(
                uid = %ctx.uid,
                mask = %shun.mask,
                reason = ?shun.reason,
                "Shunned user attempted to send message"
            );
            true
        }
        _ => false,
    }
}

// ============================================================================
// Unified Message Routing
// ============================================================================

/// Result of attempting to route a message to a channel.
enum ChannelRouteResult {
    /// Message was successfully broadcast to channel members.
    Sent,
    /// Channel does not exist.
    NoSuchChannel,
    /// Sender is blocked by +n (no external messages).
    BlockedExternal,
    /// Sender is blocked by +m (moderated).
    BlockedModerated,
}

/// Options for message routing behavior.
struct RouteOptions {
    /// Whether to check +m moderated mode (PRIVMSG/NOTICE do, TAGMSG doesn't).
    check_moderated: bool,
    /// Whether to send RPL_AWAY for user targets (only PRIVMSG).
    send_away_reply: bool,
}

/// Check if sender can speak in a channel, and broadcast if allowed.
///
/// Returns the result of the routing attempt for the caller to handle errors.
async fn route_to_channel(
    ctx: &Context<'_>,
    channel_lower: &str,
    msg: Message,
    opts: &RouteOptions,
) -> ChannelRouteResult {
    let Some(channel_ref) = ctx.matrix.channels.get(channel_lower) else {
        return ChannelRouteResult::NoSuchChannel;
    };

    let channel = channel_ref.read().await;
    let is_member = channel.is_member(ctx.uid);

    // Check +n (no external messages)
    if channel.modes.no_external && !is_member {
        return ChannelRouteResult::BlockedExternal;
    }

    // Build user's hostmask for ban/quiet checks (nick!user@host)
    let user_mask = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        format!("{}!{}@{}", user.nick, user.user, user.host)
    } else {
        // Shouldn't happen for registered users, but provide fallback
        "unknown!unknown@unknown".to_string()
    };

    // Check +b (bans) - banned users cannot speak even if in channel
    let is_banned = channel
        .bans
        .iter()
        .any(|entry| matches_hostmask(&entry.mask, &user_mask));

    if is_banned {
        // Check if user has ban exception (+e)
        let has_exception = channel
            .excepts
            .iter()
            .any(|entry| matches_hostmask(&entry.mask, &user_mask));

        if !has_exception {
            return ChannelRouteResult::BlockedExternal; // Reuse for ban
        }
    }

    // Check +q (quiet) - quieted users cannot speak
    let is_quieted = channel
        .quiets
        .iter()
        .any(|entry| matches_hostmask(&entry.mask, &user_mask));

    if is_quieted {
        // Check if user has ban exception (+e) - some IRCds allow +e to bypass +q
        let has_exception = channel
            .excepts
            .iter()
            .any(|entry| matches_hostmask(&entry.mask, &user_mask));

        if !has_exception {
            return ChannelRouteResult::BlockedModerated; // Reuse for quiet
        }
    }

    // Check +m (moderated) - only if option enabled
    if opts.check_moderated && channel.modes.moderated {
        let can_speak = channel
            .members
            .get(ctx.uid)
            .is_some_and(|m| m.op || m.voice);
        if !can_speak {
            return ChannelRouteResult::BlockedModerated;
        }
    }

    // Broadcast to all channel members except sender
    for uid in channel.members.keys() {
        if uid.as_str() == ctx.uid {
            continue;
        }
        if let Some(sender) = ctx.matrix.senders.get(uid) {
            let _ = sender.send(msg.clone()).await;
        }
    }

    ChannelRouteResult::Sent
}

/// Route a message to a user target, optionally sending RPL_AWAY.
///
/// Returns true if the user was found and message sent, false otherwise.
async fn route_to_user(
    ctx: &Context<'_>,
    target_lower: &str,
    msg: Message,
    opts: &RouteOptions,
    sender_nick: &str,
) -> bool {
    let Some(target_uid) = ctx.matrix.nicks.get(target_lower) else {
        return false;
    };

    // Check away status and notify sender if requested
    if opts.send_away_reply
        && let Some(target_user_ref) = ctx.matrix.users.get(target_uid.value())
    {
        let target_user = target_user_ref.read().await;
        if let Some(away_msg) = &target_user.away {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::RPL_AWAY,
                vec![
                    sender_nick.to_string(),
                    target_user.nick.clone(),
                    away_msg.clone(),
                ],
            );
            let _ = ctx.sender.send(reply).await;
        }
    }

    // Send to target user
    if let Some(sender) = ctx.matrix.senders.get(target_uid.value()) {
        let _ = sender.send(msg).await;
        true
    } else {
        false
    }
}

/// Send ERR_CANNOTSENDTOCHAN with the given reason.
async fn send_cannot_send(
    ctx: &Context<'_>,
    nick: &str,
    target: &str,
    reason: &str,
) -> HandlerResult {
    let reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::ERR_CANNOTSENDTOCHAN,
        vec![nick.to_string(), target.to_string(), reason.to_string()],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Send ERR_NOSUCHCHANNEL.
async fn send_no_such_channel(ctx: &Context<'_>, nick: &str, target: &str) -> HandlerResult {
    let reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::ERR_NOSUCHCHANNEL,
        vec![
            nick.to_string(),
            target.to_string(),
            "No such channel".to_string(),
        ],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Send ERR_NOSUCHNICK.
async fn send_no_such_nick(ctx: &Context<'_>, nick: &str, target: &str) -> HandlerResult {
    let reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::ERR_NOSUCHNICK,
        vec![
            nick.to_string(),
            target.to_string(),
            "No such nick/channel".to_string(),
        ],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Check if target is a channel name.
fn is_channel(target: &str) -> bool {
    matches!(target.chars().next(), Some('#' | '&' | '+' | '!'))
}

// ============================================================================
// CTCP Handling
// ============================================================================

/// Server version string for CTCP VERSION replies.
const SERVER_VERSION: &str = concat!("slircd-ng ", env!("CARGO_PKG_VERSION"));

/// Handle a CTCP request and send appropriate reply via NOTICE.
///
/// CTCP requests come in PRIVMSG, replies go out as NOTICE per spec.
/// See: https://modern.ircdocs.horse/ctcp.html
async fn handle_ctcp_request(
    ctx: &Context<'_>,
    sender_nick: &str,
    sender_user: &str,
    target: &str,
    ctcp: &Ctcp<'_>,
) -> HandlerResult {
    // Build the reply text based on CTCP type
    let reply_text = match &ctcp.kind {
        CtcpKind::Version => {
            // Reply with server version info
            Some(format!("\x01VERSION {}\x01", SERVER_VERSION))
        }
        CtcpKind::Ping => {
            // Echo back the ping timestamp
            if let Some(timestamp) = ctcp.params {
                Some(format!("\x01PING {}\x01", timestamp))
            } else {
                Some("\x01PING\x01".to_string())
            }
        }
        CtcpKind::Time => {
            // Reply with current server time
            let now = Local::now();
            Some(format!("\x01TIME {}\x01", now.format("%a %b %d %H:%M:%S %Y")))
        }
        CtcpKind::Clientinfo => {
            // List supported CTCP commands
            Some("\x01CLIENTINFO ACTION PING TIME VERSION\x01".to_string())
        }
        CtcpKind::Action => {
            // ACTION is not a request - it's a message type, relay it normally
            // Return None to fall through to normal message routing
            None
        }
        _ => {
            // Unknown CTCP - ignore silently
            debug!(ctcp = ?ctcp.kind, "Ignoring unknown CTCP request");
            return Ok(());
        }
    };

    // If we have a reply, send it via NOTICE
    if let Some(text) = reply_text {
        let target_lower = irc_to_lower(target);

        // Find target UID to send reply to
        if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower)
            && let Some(sender) = ctx.matrix.senders.get(target_uid.value())
        {
            let reply_msg = Message {
                tags: None,
                prefix: Some(user_prefix(sender_nick, sender_user, "localhost")),
                command: Command::NOTICE(target.to_string(), text),
            };
            let _ = sender.send(reply_msg).await;
            debug!(from = %sender_nick, to = %target, ctcp = ?ctcp.kind, "CTCP reply sent");
        }
        return Ok(());
    }

    // For ACTION, we need to relay the message normally
    // Return a special indicator that we should continue processing
    // Actually, we'll handle ACTION by NOT matching it above and letting it fall through
    // But since we're in this function, we need to handle it here

    // For ACTION messages, route them as normal PRIVMSG
    if matches!(ctcp.kind, CtcpKind::Action) {
        let action_text = ctcp.params.unwrap_or("");
        let full_text = format!("\x01ACTION {}\x01", action_text);

        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(sender_nick, sender_user, "localhost")),
            command: Command::PRIVMSG(target.to_string(), full_text),
        };

        let target_lower = irc_to_lower(target);
        let opts = RouteOptions {
            check_moderated: true,
            send_away_reply: true,
        };

        if route_to_user(ctx, &target_lower, out_msg, &opts, sender_nick).await {
            debug!(from = %sender_nick, to = %target, "CTCP ACTION to user");
        } else {
            send_no_such_nick(ctx, sender_nick, target).await?;
        }
    }

    Ok(())
}

// ============================================================================
// PRIVMSG Handler
// ============================================================================

/// Handler for PRIVMSG command.
pub struct PrivmsgHandler;

#[async_trait]
impl Handler for PrivmsgHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // Check shun first - silently ignore if shunned
        if is_shunned(ctx).await {
            return Ok(());
        }

        // PRIVMSG <target> <text>
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        if target.is_empty() || text.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx
            .handshake
            .user
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Check if this is a service message (NickServ, ChanServ, etc.)
        let target_lower = irc_to_lower(target);
        if (target_lower == "nickserv" || target_lower == "ns")
            && route_service_message(ctx.matrix, ctx.db, ctx.uid, nick, target, text, ctx.sender)
                .await
        {
            return Ok(());
        }
        if (target_lower == "chanserv" || target_lower == "cs")
            && route_chanserv_message(ctx.matrix, ctx.db, ctx.uid, nick, target, text, ctx.sender)
                .await
        {
            return Ok(());
        }

        // Handle CTCP requests (only for user-to-user, not channels)
        // CTCP messages start and end with \x01
        if !is_channel(target)
            && Ctcp::is_ctcp(text)
            && let Some(ctcp) = Ctcp::parse(text)
        {
            return handle_ctcp_request(ctx, nick, user_name, target, &ctcp).await;
        }

        // Build the outgoing message
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::PRIVMSG(target.to_string(), text.to_string()),
        };

        let opts = RouteOptions {
            check_moderated: true,
            send_away_reply: true,
        };

        if is_channel(target) {
            let channel_lower = irc_to_lower(target);
            match route_to_channel(ctx, &channel_lower, out_msg, &opts).await {
                ChannelRouteResult::Sent => {
                    debug!(from = %nick, to = %target, "PRIVMSG to channel");
                }
                ChannelRouteResult::NoSuchChannel => {
                    send_no_such_channel(ctx, nick, target).await?;
                }
                ChannelRouteResult::BlockedExternal => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+n)").await?;
                }
                ChannelRouteResult::BlockedModerated => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+m)").await?;
                }
            }
        } else if route_to_user(ctx, &target_lower, out_msg, &opts, nick).await {
            debug!(from = %nick, to = %target, "PRIVMSG to user");
        } else {
            send_no_such_nick(ctx, nick, target).await?;
        }

        Ok(())
    }
}

// ============================================================================
// NOTICE Handler
// ============================================================================

/// Handler for NOTICE command.
///
/// Per RFC 2812, NOTICE errors are silently ignored (no error replies).
pub struct NoticeHandler;

#[async_trait]
impl Handler for NoticeHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // Check shun first - silently ignore if shunned
        if is_shunned(ctx).await {
            return Ok(());
        }

        // NOTICE <target> <text>
        let target = msg.arg(0).unwrap_or("");
        let text = msg.arg(1).unwrap_or("");

        if target.is_empty() || text.is_empty() {
            // NOTICE errors are silently ignored per RFC
            return Ok(());
        }

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx
            .handshake
            .user
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Build the outgoing message
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::NOTICE(target.to_string(), text.to_string()),
        };

        // NOTICE: silently drop on errors, check moderated, no away reply
        let opts = RouteOptions {
            check_moderated: true,
            send_away_reply: false,
        };

        if is_channel(target) {
            let channel_lower = irc_to_lower(target);
            if let ChannelRouteResult::Sent =
                route_to_channel(ctx, &channel_lower, out_msg, &opts).await
            {
                debug!(from = %nick, to = %target, "NOTICE to channel");
            }
            // All errors silently ignored for NOTICE
        } else {
            let target_lower = irc_to_lower(target);
            if route_to_user(ctx, &target_lower, out_msg, &opts, nick).await {
                debug!(from = %nick, to = %target, "NOTICE to user");
            }
            // User not found: silently ignored for NOTICE
        }

        Ok(())
    }
}

// ============================================================================
// TAGMSG Handler
// ============================================================================

/// Handler for TAGMSG command.
///
/// IRCv3 message-tags: sends a message with only tags (no text body).
/// Requires the "message-tags" capability to be enabled.
pub struct TagmsgHandler;

#[async_trait]
impl Handler for TagmsgHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // Check shun first - silently ignore if shunned
        if is_shunned(ctx).await {
            return Ok(());
        }

        // Check if client has message-tags capability
        if !ctx.handshake.capabilities.contains("message-tags") {
            debug!("TAGMSG ignored: client lacks message-tags capability");
            return Ok(());
        }

        // TAGMSG <target>
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        if target.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx
            .handshake
            .user
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Convert tags from MessageRef to owned Tag structs
        let tags: Option<Vec<Tag>> = if msg.tags.is_some() {
            Some(
                msg.tags_iter()
                    .map(|(k, v)| {
                        let value = if v.is_empty() {
                            None
                        } else {
                            Some(v.to_string())
                        };
                        Tag(Cow::Owned(k.to_string()), value)
                    })
                    .collect(),
            )
        } else {
            None
        };

        // Build the outgoing TAGMSG
        let out_msg = Message {
            tags,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::TAGMSG(target.to_string()),
        };

        // TAGMSG: send errors, but don't check +m (only +n), no away reply
        let opts = RouteOptions {
            check_moderated: false,
            send_away_reply: false,
        };

        if is_channel(target) {
            let channel_lower = irc_to_lower(target);
            match route_to_channel(ctx, &channel_lower, out_msg, &opts).await {
                ChannelRouteResult::Sent => {
                    debug!(from = %nick, to = %target, "TAGMSG to channel");
                }
                ChannelRouteResult::NoSuchChannel => {
                    send_no_such_channel(ctx, nick, target).await?;
                }
                ChannelRouteResult::BlockedExternal => {
                    send_cannot_send(ctx, nick, target, "Cannot send to channel (+n)").await?;
                }
                ChannelRouteResult::BlockedModerated => {
                    // TAGMSG doesn't check +m, so this shouldn't happen
                    unreachable!("TAGMSG should not check moderated mode");
                }
            }
        } else {
            let target_lower = irc_to_lower(target);
            if route_to_user(ctx, &target_lower, out_msg, &opts, nick).await {
                debug!(from = %nick, to = %target, "TAGMSG to user");
            } else {
                send_no_such_nick(ctx, nick, target).await?;
            }
        }

        Ok(())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use slirc_proto::ctcp::{Ctcp, CtcpKind};

    #[test]
    fn test_is_channel() {
        assert!(is_channel("#rust"));
        assert!(is_channel("&local"));
        assert!(is_channel("+modeless"));
        assert!(is_channel("!safe"));
        assert!(!is_channel("nickname"));
        assert!(!is_channel("NickServ"));
    }

    #[test]
    fn test_matches_hostmask_exact() {
        assert!(matches_hostmask("nick!user@host", "nick!user@host"));
        assert!(!matches_hostmask("nick!user@host", "other!user@host"));
    }

    #[test]
    fn test_matches_hostmask_wildcard_star() {
        assert!(matches_hostmask("*!*@*", "nick!user@host"));
        assert!(matches_hostmask("nick!*@*", "nick!user@host"));
        assert!(matches_hostmask("*!user@*", "nick!user@host"));
        assert!(matches_hostmask("*!*@host", "nick!user@host"));
        assert!(matches_hostmask("*!*@*.example.com", "nick!user@sub.example.com"));
    }

    #[test]
    fn test_matches_hostmask_wildcard_question() {
        assert!(matches_hostmask("nic?!user@host", "nick!user@host"));
        assert!(matches_hostmask("????!user@host", "nick!user@host"));
        assert!(!matches_hostmask("???!user@host", "nick!user@host"));
    }

    #[test]
    fn test_matches_hostmask_case_insensitive() {
        assert!(matches_hostmask("NICK!USER@HOST", "nick!user@host"));
        assert!(matches_hostmask("Nick!User@Host", "NICK!USER@HOST"));
    }

    #[test]
    fn test_ctcp_parsing() {
        // Verify slirc_proto's CTCP parsing works as expected
        let version = Ctcp::parse("\x01VERSION\x01");
        assert!(version.is_some());
        assert!(matches!(version.unwrap().kind, CtcpKind::Version));

        let ping = Ctcp::parse("\x01PING 1234567890\x01");
        assert!(ping.is_some());
        let ping = ping.unwrap();
        assert!(matches!(ping.kind, CtcpKind::Ping));
        assert_eq!(ping.params, Some("1234567890"));

        let time = Ctcp::parse("\x01TIME\x01");
        assert!(time.is_some());
        assert!(matches!(time.unwrap().kind, CtcpKind::Time));

        let clientinfo = Ctcp::parse("\x01CLIENTINFO\x01");
        assert!(clientinfo.is_some());
        assert!(matches!(clientinfo.unwrap().kind, CtcpKind::Clientinfo));

        let action = Ctcp::parse("\x01ACTION waves\x01");
        assert!(action.is_some());
        let action = action.unwrap();
        assert!(matches!(action.kind, CtcpKind::Action));
        assert_eq!(action.params, Some("waves"));
    }

    #[test]
    fn test_ctcp_is_ctcp() {
        assert!(Ctcp::is_ctcp("\x01VERSION\x01"));
        assert!(Ctcp::is_ctcp("\x01ACTION test\x01"));
        assert!(!Ctcp::is_ctcp("regular message"));
        // slirc_proto is lenient: strings starting with \x01 are considered CTCP
        // and parse() accepts messages without trailing \x01 (real-world tolerance)
        assert!(Ctcp::is_ctcp("\x01incomplete"));
        assert!(Ctcp::parse("\x01incomplete").is_some()); // Lenient parsing
    }

    #[test]
    fn test_server_version_constant() {
        // Ensure SERVER_VERSION is set correctly
        assert!(SERVER_VERSION.starts_with("slircd-ng "));
        assert!(SERVER_VERSION.contains(env!("CARGO_PKG_VERSION")));
    }
}
