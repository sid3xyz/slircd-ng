//! Messaging handlers.
//!
//! Handles PRIVMSG and NOTICE commands for both users and channels.

use super::{server_reply, Context, Handler, HandlerError, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, Prefix, Response};
use tracing::debug;

/// Helper to create a user prefix.
fn user_prefix(nick: &str, user: &str, host: &str) -> Prefix {
    Prefix::Nickname(nick.to_string(), user.to_string(), host.to_string())
}

/// Handler for PRIVMSG command.
pub struct PrivmsgHandler;

#[async_trait]
impl Handler for PrivmsgHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        let (target, text) = match &msg.command {
            Command::PRIVMSG(t, txt) => (t.clone(), txt.clone()),
            _ => return Ok(()),
        };

        if target.is_empty() || text.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let nick = ctx.handshake.nick.as_ref().unwrap();
        let user_name = ctx.handshake.user.as_ref().unwrap();

        // Build the outgoing message with user prefix
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::PRIVMSG(target.clone(), text),
        };

        // Is it a channel or a user?
        if target.starts_with('#') || target.starts_with('&') {
            // Channel message
            let channel_lower = irc_to_lower(&target);

            // Check if channel exists
            if let Some(channel) = ctx.matrix.channels.get(&channel_lower) {
                let channel = channel.read().await;

                // Check if user is in channel (for now, allow non-members to send)
                // TODO: Add +n mode check

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
                        target,
                        "No such channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
        } else {
            // User message
            let target_lower = irc_to_lower(&target);

            if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
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
                        target,
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
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        let (target, text) = match &msg.command {
            Command::NOTICE(t, txt) => (t.clone(), txt.clone()),
            _ => return Ok(()),
        };

        if target.is_empty() || text.is_empty() {
            // NOTICE errors are silently ignored per RFC
            return Ok(());
        }

        let nick = ctx.handshake.nick.as_ref().unwrap();
        let user_name = ctx.handshake.user.as_ref().unwrap();

        // Build the outgoing message with user prefix
        let out_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::NOTICE(target.clone(), text),
        };

        // Is it a channel or a user?
        if target.starts_with('#') || target.starts_with('&') {
            // Channel notice
            let channel_lower = irc_to_lower(&target);

            if let Some(channel) = ctx.matrix.channels.get(&channel_lower) {
                let channel = channel.read().await;

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
            let target_lower = irc_to_lower(&target);

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
