//! Messaging handlers.
//!
//! Handles PRIVMSG and NOTICE commands for both users and channels.

use super::{server_reply, user_prefix, Context, Handler, HandlerError, HandlerResult};
use crate::services::chanserv::route_chanserv_message;
use crate::services::nickserv::route_service_message;
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, MessageRef, Response};
use tracing::debug;

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
            // Route to NickServ
            if route_service_message(
                ctx.matrix,
                ctx.db,
                ctx.uid,
                nick,
                target,
                text,
                ctx.sender,
            ).await {
                return Ok(());
            }
        }

        if target_lower == "chanserv" || target_lower == "cs" {
            // Route to ChanServ
            if route_chanserv_message(
                ctx.matrix,
                ctx.db,
                ctx.uid,
                nick,
                target,
                text,
                ctx.sender,
            ).await {
                return Ok(());
            }
        }

        // Build the outgoing message with user prefix
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::PRIVMSG(target.to_string(), text.to_string()),
        };

        // Is it a channel or a user?
        if matches!(target.chars().next(), Some('#' | '&' | '+' | '!')) {
            // Channel message
            let channel_lower = irc_to_lower(target);

            // Check if channel exists
            if let Some(channel) = ctx.matrix.channels.get(&channel_lower) {
                let channel = channel.read().await;
                let is_member = channel.is_member(ctx.uid);

                // Check +n (no external messages) - non-members cannot send
                if channel.modes.no_external && !is_member {
                    let reply = server_reply(
                        &ctx.matrix.server_info.name,
                        Response::ERR_CANNOTSENDTOCHAN,
                        vec![
                            nick.to_string(),
                            target.to_string(),
                            "Cannot send to channel (+n)".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                    return Ok(());
                }

                // Check +m (moderated) - only ops/voice can speak
                if channel.modes.moderated {
                    let member_modes = channel.members.get(ctx.uid);
                    let can_speak = member_modes.is_some_and(|m| m.op || m.voice);
                    if !can_speak {
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::ERR_CANNOTSENDTOCHAN,
                            vec![
                                nick.to_string(),
                                target.to_string(),
                                "Cannot send to channel (+m)".to_string(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                        return Ok(());
                    }
                }

                // Broadcast to all channel members except sender
                for uid in channel.members.keys() {
                    if uid.as_str() == ctx.uid {
                        continue; // Don't echo back to sender
                    }
                    if let Some(sender) = ctx.matrix.senders.get(uid) {
                        let _ = sender.send(out_msg.clone()).await;
                    }
                }

                debug!(from = %nick, to = %target, "PRIVMSG to channel");
            } else {
                // Channel doesn't exist
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
            }
        } else {
            // User message
            let target_lower = irc_to_lower(target);

            if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                // Check if target is away and notify sender
                if let Some(target_user_ref) = ctx.matrix.users.get(target_uid.value()) {
                    let target_user = target_user_ref.read().await;
                    if let Some(away_msg) = &target_user.away {
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_AWAY,
                            vec![
                                nick.to_string(),
                                target_user.nick.clone(),
                                away_msg.clone(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                    }
                }

                // Send to target user
                if let Some(sender) = ctx.matrix.senders.get(target_uid.value()) {
                    let _ = sender.send(out_msg).await;
                    debug!(from = %nick, to = %target, "PRIVMSG to user");
                }
            } else {
                // User not found
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
            }
        }

        Ok(())
    }
}

/// Handler for NOTICE command.
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

        // Build the outgoing message with user prefix
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::NOTICE(target.to_string(), text.to_string()),
        };

        // Is it a channel or a user?
        if matches!(target.chars().next(), Some('#' | '&' | '+' | '!')) {
            // Channel notice
            let channel_lower = irc_to_lower(target);

            if let Some(channel) = ctx.matrix.channels.get(&channel_lower) {
                let channel = channel.read().await;
                let is_member = channel.is_member(ctx.uid);

                // Check +n (no external messages) - silently drop per NOTICE semantics
                if channel.modes.no_external && !is_member {
                    return Ok(());
                }

                // Check +m (moderated) - silently drop per NOTICE semantics
                if channel.modes.moderated {
                    let member_modes = channel.members.get(ctx.uid);
                    let can_speak = member_modes.is_some_and(|m| m.op || m.voice);
                    if !can_speak {
                        return Ok(());
                    }
                }

                // Broadcast to all channel members except sender
                for uid in channel.members.keys() {
                    if uid.as_str() == ctx.uid {
                        continue;
                    }
                    if let Some(sender) = ctx.matrix.senders.get(uid) {
                        let _ = sender.send(out_msg.clone()).await;
                    }
                }

                debug!(from = %nick, to = %target, "NOTICE to channel");
            }
            // No error for non-existent channel (per NOTICE semantics)
        } else {
            // User notice
            let target_lower = irc_to_lower(target);

            if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower)
                && let Some(sender) = ctx.matrix.senders.get(target_uid.value())
            {
                let _ = sender.send(out_msg).await;
                debug!(from = %nick, to = %target, "NOTICE to user");
            }
            // No error for non-existent user (per NOTICE semantics)
        }

        Ok(())
    }
}

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
            // Silently ignore TAGMSG from clients without the capability
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
        // tags_iter() yields (key, value) pairs
        let tags: Option<Vec<slirc_proto::Tag>> = if msg.tags.is_some() {
            Some(
                msg.tags_iter()
                    .map(|(k, v)| {
                        let value = if v.is_empty() { None } else { Some(v.to_string()) };
                        slirc_proto::Tag(std::borrow::Cow::Owned(k.to_string()), value)
                    })
                    .collect()
            )
        } else {
            None
        };

        // Build the outgoing TAGMSG with user prefix
        let out_msg = Message {
            tags,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::TAGMSG(target.to_string()),
        };

        // Is it a channel or a user?
        if matches!(target.chars().next(), Some('#' | '&' | '+' | '!')) {
            // Channel TAGMSG
            let channel_lower = irc_to_lower(target);

            if let Some(channel) = ctx.matrix.channels.get(&channel_lower) {
                let channel = channel.read().await;
                let is_member = channel.is_member(ctx.uid);

                // Check +n (no external messages) - non-members cannot send
                if channel.modes.no_external && !is_member {
                    let reply = server_reply(
                        &ctx.matrix.server_info.name,
                        Response::ERR_CANNOTSENDTOCHAN,
                        vec![
                            nick.to_string(),
                            target.to_string(),
                            "Cannot send to channel (+n)".to_string(),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                    return Ok(());
                }

                // Broadcast to all channel members with message-tags capability, except sender
                for uid in channel.members.keys() {
                    if uid.as_str() == ctx.uid {
                        continue; // Don't echo back to sender
                    }
                    // Only send to clients with message-tags capability
                    // Note: In a full implementation, we'd track caps per-user
                    // For now, we send to all members (they'll parse it correctly)
                    if let Some(sender) = ctx.matrix.senders.get(uid) {
                        let _ = sender.send(out_msg.clone()).await;
                    }
                }

                debug!(from = %nick, to = %target, "TAGMSG to channel");
            } else {
                // Channel doesn't exist
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
            }
        } else {
            // User TAGMSG
            let target_lower = irc_to_lower(target);

            if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
                // Send to target user
                if let Some(sender) = ctx.matrix.senders.get(target_uid.value()) {
                    let _ = sender.send(out_msg).await;
                    debug!(from = %nick, to = %target, "TAGMSG to user");
                }
            } else {
                // User not found
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
            }
        }

        Ok(())
    }
}
