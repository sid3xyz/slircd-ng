//! Common utilities for message handling.
//!
//! Shared helpers for PRIVMSG, NOTICE, and TAGMSG handlers including shun checking,
//! routing logic, channel validation, and error responses.

use super::super::{Context, HandlerResult, server_reply};
use crate::security::UserContext;
use crate::security::spam::SpamVerdict;
use slirc_proto::ctcp::{Ctcp, CtcpKind};
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

pub use crate::state::actor::ChannelRouteResult;

/// Options for message routing behavior.
pub struct RouteOptions {
    /// Whether to send RPL_AWAY for user targets (only PRIVMSG).
    pub send_away_reply: bool,
    /// Whether this is a NOTICE (for +T check).
    pub is_notice: bool,
    /// Whether to block CTCP (+C mode, except ACTION).
    #[allow(dead_code)] // Reserved for future use
    pub block_ctcp: bool,
    /// Status prefix for channel messages (e.g. @#chan).
    pub status_prefix: Option<char>,
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

    // Get user info
    let (user_context, is_registered) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user = user_ref.read().await;
        let context = UserContext::for_registration(
            ctx.remote_addr.ip(),
            user.host.clone(),
            user.nick.clone(),
            user.user.clone(),
            user.realname.clone(),
            ctx.matrix.server_info.name.clone(),
            user.account.clone(),
        );
        (context, user.modes.registered)
    } else {
        // Fallback for unregistered users (shouldn't happen usually)
        let context = UserContext::for_registration(
            ctx.remote_addr.ip(),
            "unknown".to_string(),
            "unknown".to_string(),
            "unknown".to_string(),
            "unknown".to_string(),
            ctx.matrix.server_info.name.clone(),
            None,
        );
        (context, false)
    };

        // Extract text and tags from message
    let (text, tags) = match &msg.command {
        Command::PRIVMSG(_, text) | Command::NOTICE(_, text) => (text.clone(), msg.tags.clone()),
        _ => return ChannelRouteResult::Sent, // Should not happen
    };

    let is_notice = matches!(msg.command, Command::NOTICE(_, _));

    // Send to actor
    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let event = crate::state::actor::ChannelEvent::Message {
        sender_uid: ctx.uid.to_string(),
        text,
        tags,
        is_notice,
        user_context: Box::new(user_context),
        is_registered,
        is_tls: ctx.handshake.is_tls,
        status_prefix: opts.status_prefix,
        reply_tx,
    };

    if let Err(_) = channel_ref.send(event).await {
        return ChannelRouteResult::NoSuchChannel; // Actor died
    }

    match reply_rx.await {
        Ok(result) => result,
        Err(_) => ChannelRouteResult::NoSuchChannel,
    }
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

    // Spam detection for direct messages (skip TAGMSG).
    if let Some(detector) = &ctx.matrix.spam_detector
        && let Some(text) = match &msg.command {
            Command::PRIVMSG(_, text) | Command::NOTICE(_, text) => Some(text.as_str()),
            _ => None,
        } {
            // Check trust level
            let is_trusted = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                user.modes.oper || user.account.is_some()
            } else {
                false
            };

            if !is_trusted
                && let SpamVerdict::Spam { pattern, .. } = detector.check_message(text) {
                    if !opts.is_notice {
                        let _ = send_cannot_send(
                            ctx,
                            sender_nick,
                            target_lower,
                            "Message rejected as spam",
                        )
                        .await;
                    }
                    debug!(pattern = %pattern, "Direct message blocked as spam");
                    return false;
                }
        }

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
                format!(
                    "{}!{}@{}",
                    sender_user.nick, sender_user.user, sender_user.visible_host
                )
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

        // Check +T (no CTCP) - block CTCP messages except ACTION
        if target_user.modes.no_ctcp {
            // Extract text from command to check for CTCP
            let text = match &msg.command {
                Command::PRIVMSG(_, text) | Command::NOTICE(_, text) => Some(text.as_str()),
                _ => None,
            };
            if let Some(text) = text
                && Ctcp::is_ctcp(text)
            {
                // Check if it's an ACTION (allowed even with +T)
                if let Some(ctcp) = Ctcp::parse(text)
                    && !matches!(ctcp.kind, CtcpKind::Action)
                {
                    debug!(
                        target = %target_user.nick,
                        ctcp_type = ?ctcp.kind,
                        "CTCP blocked by +T mode"
                    );
                    return false; // Silently drop non-ACTION CTCP
                }
            }
        }
    }

    // Send to target user with appropriate tags based on their capabilities
    let timestamp = chrono::Utc::now()
        .format("%Y-%m-%dT%H:%M:%S%.3fZ")
        .to_string();
    let msgid = uuid::Uuid::new_v4().to_string();

    // Check if this is a TAGMSG
    let is_tagmsg = matches!(msg.command, Command::TAGMSG(_));

    if let Some(sender) = ctx.matrix.senders.get(target_uid.value()) {
        // Check target's capabilities and build appropriate message
        let msg_for_target = if let Some(user_ref) = ctx.matrix.users.get(target_uid.value()) {
            let user = user_ref.read().await;
            let has_message_tags = user.caps.contains("message-tags");
            let has_server_time = user.caps.contains("server-time");

            let mut result = msg.clone();

            // Strip label tag from recipient copies (label is sender-only)
            if ctx.label.is_some() {
                result.tags = result
                    .tags
                    .map(|tags| {
                        tags.into_iter()
                            .filter(|tag| tag.0.as_ref() != "label")
                            .collect::<Vec<_>>()
                    })
                    .and_then(|tags| if tags.is_empty() { None } else { Some(tags) });
            }

            // If recipient doesn't have message-tags, strip client-only tags
            if !has_message_tags {
                result.tags = result
                    .tags
                    .map(|tags| {
                        tags.into_iter()
                            .filter(|tag| !tag.0.starts_with('+'))
                            .collect::<Vec<_>>()
                    })
                    .and_then(|tags| if tags.is_empty() { None } else { Some(tags) });
            } else {
                // Add msgid for users with message-tags
                result = result.with_tag("msgid", Some(msgid.clone()));
            }

            // Add server-time if capability is enabled
            if has_server_time {
                result = result.with_tag("time", Some(timestamp.clone()));
            }

            result
        } else {
            msg.clone()
        };
        let _ = sender.send(msg_for_target).await;
        crate::metrics::MESSAGES_SENT.inc();

        // Echo message back to sender if they have echo-message capability
        if ctx.handshake.capabilities.contains("echo-message") {
            let has_message_tags = ctx.handshake.capabilities.contains("message-tags");
            let has_server_time = ctx.handshake.capabilities.contains("server-time");
            let has_labeled_response = ctx.label.is_some();

            let mut echo_msg = msg.clone();

            // For labeled-response with PRIVMSG/NOTICE: strip ALL client tags
            // For TAGMSG: preserve client-only tags (they're the whole point!)
            if has_labeled_response && !is_tagmsg {
                echo_msg.tags = None; // Start fresh, only add server tags below
            }

            // Add msgid if sender has message-tags
            if has_message_tags {
                echo_msg = echo_msg.with_tag("msgid", Some(msgid));
            }

            // Add server-time if capability is enabled
            if has_server_time {
                echo_msg = echo_msg.with_tag("time", Some(timestamp));
            }

            // Preserve label if present
            if let Some(ref label) = ctx.label {
                echo_msg = echo_msg.with_tag("label", Some(label.clone()));
            }

            let _ = ctx.sender.send(echo_msg).await;
        }

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
