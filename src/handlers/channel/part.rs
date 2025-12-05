//! PART command handler.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_notonchannel, server_reply,
    user_mask_from_state, user_prefix,
};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Response, irc_to_lower};
use tracing::{debug, info};

/// Handler for PART command.
pub struct PartHandler;

#[async_trait]
impl Handler for PartHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // PART <channels> [reason]
        let channels_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let reason = msg.arg(1);

        let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        for channel_name in channels_str.split(',') {
            let channel_name = channel_name.trim();
            if channel_name.is_empty() {
                continue;
            }

            let channel_lower = irc_to_lower(channel_name);
            leave_channel_internal(ctx, &channel_lower, &nick, &user_name, &host, reason).await?;
        }

        Ok(())
    }
}

/// Internal function to leave a channel.
pub(super) async fn leave_channel_internal(
    ctx: &mut Context<'_>,
    channel_lower: &str,
    nick: &str,
    user_name: &str,
    host: &str,
    reason: Option<&str>,
) -> HandlerResult {
    // Check if channel exists
    let channel = match ctx.matrix.channels.get(channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOSUCHCHANNEL,
                vec![
                    nick.to_string(),
                    channel_lower.to_string(),
                    "No such channel".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }
    };

    let mut channel_guard = channel.write().await;

    // Check if user is in channel
    if !channel_guard.is_member(ctx.uid) {
        ctx.sender
            .send(err_notonchannel(
                &ctx.matrix.server_info.name,
                nick,
                &channel_guard.name,
            ))
            .await?;
        return Ok(());
    }

    let canonical_name = channel_guard.name.clone();

    // Broadcast PART before removing
    let part_msg = Message {
        tags: None,
        prefix: Some(user_prefix(nick, user_name, host)),
        command: Command::PART(canonical_name.clone(), reason.map(String::from)),
    };

    // Broadcast to all members including self
    for uid in channel_guard.members.keys() {
        if let Some(sender) = ctx.matrix.senders.get(uid) {
            let _ = sender.send(part_msg.clone()).await;
        }
    }

    // Remove user from channel
    channel_guard.remove_member(ctx.uid);
    let is_empty = channel_guard.members.is_empty();
    let is_permanent = channel_guard.modes.permanent;

    drop(channel_guard);

    // Remove channel from user's list
    if let Some(user) = ctx.matrix.users.get(ctx.uid) {
        let mut user = user.write().await;
        user.channels.remove(channel_lower);
    }

    // If channel is now empty and not permanent (+P), remove it
    if is_empty && !is_permanent {
        ctx.matrix.channels.remove(channel_lower);
        crate::metrics::ACTIVE_CHANNELS.dec();
        debug!(channel = %canonical_name, "Channel removed (empty)");
    } else if is_empty && is_permanent {
        debug!(channel = %canonical_name, "Channel kept alive (+P permanent)");
    }

    info!(nick = %nick, channel = %canonical_name, "User left channel");

    Ok(())
}
