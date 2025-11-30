//! Messaging handlers.
//!
//! Handles PRIVMSG, NOTICE, and TAGMSG commands for both users and channels.
//! Uses a unified routing system to enforce channel modes (+n, +m).

use super::{server_reply, user_prefix, Context, Handler, HandlerError, HandlerResult};
use crate::services::chanserv::route_chanserv_message;
use crate::services::nickserv::route_service_message;
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, MessageRef, Response, Tag};
use std::borrow::Cow;
use tracing::debug;

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
    let is_banned = channel.bans.iter()
        .any(|entry| matches_hostmask(&entry.mask, &user_mask));
    
    if is_banned {
        // Check if user has ban exception (+e)
        let has_exception = channel.excepts.iter()
            .any(|entry| matches_hostmask(&entry.mask, &user_mask));
        
        if !has_exception {
            return ChannelRouteResult::BlockedExternal; // Reuse for ban
        }
    }

    // Check +q (quiet) - quieted users cannot speak
    let is_quieted = channel.quiets.iter()
        .any(|entry| matches_hostmask(&entry.mask, &user_mask));
    
    if is_quieted {
        // Check if user has ban exception (+e) - some IRCds allow +e to bypass +q
        let has_exception = channel.excepts.iter()
            .any(|entry| matches_hostmask(&entry.mask, &user_mask));
        
        if !has_exception {
            return ChannelRouteResult::BlockedModerated; // Reuse for quiet
        }
    }

    // Check +m (moderated) - only if option enabled
    if opts.check_moderated && channel.modes.moderated {
        let can_speak = channel.members.get(ctx.uid).is_some_and(|m| m.op || m.voice);
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
    if opts.send_away_reply {
        if let Some(target_user_ref) = ctx.matrix.users.get(target_uid.value()) {
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
async fn send_cannot_send(ctx: &Context<'_>, nick: &str, target: &str, reason: &str) -> HandlerResult {
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
        vec![nick.to_string(), target.to_string(), "No such channel".to_string()],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Send ERR_NOSUCHNICK.
async fn send_no_such_nick(ctx: &Context<'_>, nick: &str, target: &str) -> HandlerResult {
    let reply = server_reply(
        &ctx.matrix.server_info.name,
        Response::ERR_NOSUCHNICK,
        vec![nick.to_string(), target.to_string(), "No such nick/channel".to_string()],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Check if target is a channel name.
fn is_channel(target: &str) -> bool {
    matches!(target.chars().next(), Some('#' | '&' | '+' | '!'))
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

        // PRIVMSG <target> <text>
        let target = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        if target.is_empty() || text.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx.handshake.user.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // Check if this is a service message (NickServ, ChanServ, etc.)
        let target_lower = irc_to_lower(target);
        if target_lower == "nickserv" || target_lower == "ns" {
            if route_service_message(ctx.matrix, ctx.db, ctx.uid, nick, target, text, ctx.sender).await {
                return Ok(());
            }
        }
        if target_lower == "chanserv" || target_lower == "cs" {
            if route_chanserv_message(ctx.matrix, ctx.db, ctx.uid, nick, target, text, ctx.sender).await {
                return Ok(());
            }
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
        } else {
            if route_to_user(ctx, &target_lower, out_msg, &opts, nick).await {
                debug!(from = %nick, to = %target, "PRIVMSG to user");
            } else {
                send_no_such_nick(ctx, nick, target).await?;
            }
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

        // NOTICE <target> <text>
        let target = msg.arg(0).unwrap_or("");
        let text = msg.arg(1).unwrap_or("");

        if target.is_empty() || text.is_empty() {
            // NOTICE errors are silently ignored per RFC
            return Ok(());
        }

        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx.handshake.user.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

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
            if let ChannelRouteResult::Sent = route_to_channel(ctx, &channel_lower, out_msg, &opts).await {
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

        let nick = ctx.handshake.nick.as_ref().ok_or(HandlerError::NickOrUserMissing)?;
        let user_name = ctx.handshake.user.as_ref().ok_or(HandlerError::NickOrUserMissing)?;

        // Convert tags from MessageRef to owned Tag structs
        let tags: Option<Vec<Tag>> = if msg.tags.is_some() {
            Some(
                msg.tags_iter()
                    .map(|(k, v)| {
                        let value = if v.is_empty() { None } else { Some(v.to_string()) };
                        Tag(Cow::Owned(k.to_string()), value)
                    })
                    .collect()
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

/// Check if a hostmask (nick!user@host) matches a ban/invite pattern.
/// Supports wildcards (* and ?).
fn matches_hostmask(pattern: &str, hostmask: &str) -> bool {
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
