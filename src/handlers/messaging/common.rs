//! Common utilities for message handling.
//!
//! Shared helpers for PRIVMSG, NOTICE, and TAGMSG handlers including shun checking,
//! routing logic, channel validation, and error responses.

use super::super::{server_reply, Context, HandlerResult, matches_ban_or_except};
use crate::security::spam::SpamVerdict;
use crate::security::UserContext;
use slirc_proto::{Command, Message, Response};
use tracing::debug;

// ============================================================================
// Shun Checking
// ============================================================================

/// Check if a user is shunned (silently blocked from messaging).
///
/// Returns true if the user is shunned and their command should be silently ignored.
pub async fn is_shunned(ctx: &Context<'_>) -> bool {
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
// Message Routing Types
// ============================================================================

/// Result of attempting to route a message to a channel.
#[allow(dead_code)] // BlockedCTCP reserved for future use
pub enum ChannelRouteResult {
    /// Message was successfully broadcast to channel members.
    Sent,
    /// Channel does not exist.
    NoSuchChannel,
    /// Sender is blocked by +n (no external messages).
    BlockedExternal,
    /// Sender is blocked by +m (moderated).
    BlockedModerated,
    /// Message blocked by spam detection.
    BlockedSpam,
    /// Sender is blocked by +r (registered-only channel).
    BlockedRegisteredOnly,
    /// Blocked by +C (no CTCP except ACTION).
    BlockedCTCP,
    /// Blocked by +T (no channel NOTICE).
    BlockedNotice,
}

/// Options for message routing behavior.
pub struct RouteOptions {
    /// Whether to check +m moderated mode (PRIVMSG/NOTICE do, TAGMSG doesn't).
    pub check_moderated: bool,
    /// Whether to send RPL_AWAY for user targets (only PRIVMSG).
    pub send_away_reply: bool,
    /// Whether this is a NOTICE (for +T check).
    pub is_notice: bool,
    /// Whether to strip colors (+c mode).
    pub strip_colors: bool,
    /// Whether to block CTCP (+C mode, except ACTION).
    #[allow(dead_code)] // Reserved for future use
    pub block_ctcp: bool,
}

// ============================================================================
// Routing Functions
// ============================================================================

/// Check if sender can speak in a channel, and broadcast if allowed.
///
/// Returns the result of the routing attempt for the caller to handle errors.
pub async fn route_to_channel(
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
    let (user_mask, user_context, is_registered) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        let mask = format!("{}!{}@{}", user.nick, user.user, user.host);
        let context = UserContext::for_registration(
            ctx.remote_addr.ip(),
            user.host.clone(),
            user.nick.clone(),
            user.user.clone(),
            user.realname.clone(),
            ctx.matrix.server_info.name.clone(),
            user.account.clone(),
        );
        (mask, context, user.modes.registered)
    } else {
        // Shouldn't happen for registered users, but provide fallback
        let mask = "unknown!unknown@unknown".to_string();
        let context = UserContext::for_registration(
            ctx.remote_addr.ip(),
            "unknown".to_string(),
            "unknown".to_string(),
            "unknown".to_string(),
            "unknown".to_string(),
            ctx.matrix.server_info.name.clone(),
            None,
        );
        (mask, context, false)
    };

    // Check +r (registered-only channel)
    if channel.modes.registered_only && !is_registered {
        crate::metrics::REGISTERED_ONLY_BLOCKED.inc();
        return ChannelRouteResult::BlockedRegisteredOnly;
    }

    // Check +z (TLS-only channel)
    if channel.modes.tls_only && !ctx.handshake.is_tls {
        return ChannelRouteResult::BlockedExternal; // Reuse error type
    }

    // Check +b (bans) - banned users cannot speak even if in channel
    // Supports both hostmask and extended bans ($a:account, $r:realname, etc.)
    let is_banned = channel
        .bans
        .iter()
        .any(|entry| matches_ban_or_except(&entry.mask, &user_mask, &user_context))
        || channel
            .extended_bans
            .iter()
            .any(|entry| matches_ban_or_except(&entry.mask, &user_mask, &user_context));

    if is_banned {
        // Check if user has ban exception (+e) - supports extended bans
        let has_exception = channel
            .excepts
            .iter()
            .any(|entry| matches_ban_or_except(&entry.mask, &user_mask, &user_context));

        if !has_exception {
            return ChannelRouteResult::BlockedExternal; // Reuse for ban
        }
    }

    // Check +q (quiet) - quieted users cannot speak
    // Supports both hostmask and extended bans
    let is_quieted = channel
        .quiets
        .iter()
        .any(|entry| matches_ban_or_except(&entry.mask, &user_mask, &user_context));

    if is_quieted {
        // Check if user has ban exception (+e) - some IRCds allow +e to bypass +q
        let has_exception = channel
            .excepts
            .iter()
            .any(|entry| matches_ban_or_except(&entry.mask, &user_mask, &user_context));

        if !has_exception {
            return ChannelRouteResult::BlockedModerated; // Reuse for quiet
        }
    }

    // Check for spam (if enabled)
    if let Some(detector) = &ctx.matrix.spam_detector {
        // Extract message text from PRIVMSG command
        if let Command::PRIVMSG(_, ref text) = msg.command {
            match detector.check_message(text) {
                SpamVerdict::Spam { pattern, .. } => {
                    debug!(
                        uid = %ctx.uid,
                        channel = %channel_lower,
                        pattern = %pattern,
                        "Message blocked as spam"
                    );
                    crate::metrics::SPAM_BLOCKED.inc();
                    return ChannelRouteResult::BlockedSpam;
                }
                SpamVerdict::Clean => {
                    // Message is clean, proceed
                }
            }
        }
    }

    // Check +T (no channel NOTICE)
    if opts.is_notice && channel.modes.no_channel_notice {
        return ChannelRouteResult::BlockedNotice;
    }

    // Check +m (moderated) - only if option enabled
    if opts.check_moderated && channel.modes.moderated && !channel.can_speak(ctx.uid) {
        return ChannelRouteResult::BlockedModerated;
    }

    // Prepare final message: potentially strip colors (+c) or modify text
    let final_msg = if opts.strip_colors && channel.modes.no_colors {
        // Strip IRC formatting codes from message text
        use slirc_proto::colors::FormattedStringExt;
        match &msg.command {
            Command::PRIVMSG(target, text) => {
                let stripped = text.as_str().strip_formatting();
                Message {
                    tags: msg.tags.clone(),
                    prefix: msg.prefix.clone(),
                    command: Command::PRIVMSG(target.clone(), stripped.into_owned()),
                }
            }
            Command::NOTICE(target, text) => {
                let stripped = text.as_str().strip_formatting();
                Message {
                    tags: msg.tags.clone(),
                    prefix: msg.prefix.clone(),
                    command: Command::NOTICE(target.clone(), stripped.into_owned()),
                }
            }
            _ => msg,
        }
    } else {
        msg
    };

    // Broadcast to all channel members except sender
    for uid in channel.members.keys() {
        if uid.as_str() == ctx.uid {
            continue;
        }
        if let Some(sender) = ctx.matrix.senders.get(uid) {
            let _ = sender.send(final_msg.clone()).await;
            crate::metrics::MESSAGES_SENT.inc();
        }
    }

    ChannelRouteResult::Sent
}

/// Route a message to a user target, optionally sending RPL_AWAY.
///
/// Returns true if the user was found and message sent, false otherwise.
pub async fn route_to_user(
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

    // Check +R (registered-only PMs) - target only accepts PMs from identified users
    if let Some(target_user_ref) = ctx.matrix.users.get(target_uid.value()) {
        let target_user = target_user_ref.read().await;
        if target_user.modes.registered_only {
            // Check if sender is identified
            let sender_identified = if let Some(sender_ref) = ctx.matrix.users.get(ctx.uid) {
                let sender_user = sender_ref.read().await;
                sender_user.modes.registered
            } else {
                false
            };

            if !sender_identified {
                // Silently drop or send error - most servers silently drop
                // to avoid information leakage about +R status
                return false;
            }
        }

        // Check SILENCE list - if sender matches any mask in target's silence list, drop silently
        if !target_user.silence_list.is_empty() {
            let sender_mask = if let Some(sender_ref) = ctx.matrix.users.get(ctx.uid) {
                let sender_user = sender_ref.read().await;
                format!("{}!{}@{}", sender_user.nick, sender_user.user, sender_user.visible_host)
            } else {
                String::from("*!*@*")
            };

            for silence_mask in &target_user.silence_list {
                if super::super::matches_hostmask(silence_mask, &sender_mask) {
                    // Silently drop the message
                    debug!(
                        target = %target_user.nick,
                        sender = %sender_mask,
                        mask = %silence_mask,
                        "Message blocked by SILENCE"
                    );
                    return false;
                }
            }
        }
    }

    // Send to target user
    if let Some(sender) = ctx.matrix.senders.get(target_uid.value()) {
        let _ = sender.send(msg).await;
        crate::metrics::MESSAGES_SENT.inc();
        true
    } else {
        false
    }
}

// ============================================================================
// Error Response Helpers
// ============================================================================

/// Send ERR_CANNOTSENDTOCHAN with the given reason.
pub async fn send_cannot_send(
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
pub async fn send_no_such_channel(ctx: &Context<'_>, nick: &str, target: &str) -> HandlerResult {
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
pub async fn send_no_such_nick(ctx: &Context<'_>, nick: &str, target: &str) -> HandlerResult {
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
