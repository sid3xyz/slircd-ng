//! KICK command handler.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_chanoprivsneeded, err_usernotinchannel,
    server_reply, user_prefix,
};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Response, irc_to_lower};
use tracing::info;

/// Handler for KICK command.
pub struct KickHandler;

#[async_trait]
impl Handler for KickHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            return Err(HandlerError::NotRegistered);
        }

        // KICK <channel> <nick> [reason]
        let channel_name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let target_nick = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let reason = msg.arg(2);

        if channel_name.is_empty() || target_nick.is_empty() {
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
        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        nick.clone(),
                        channel_name.to_string(),
                        "No such channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let mut channel_guard = channel.write().await;

        // Check if kicker is op
        if !channel_guard.is_op(ctx.uid) {
            ctx.sender
                .send(err_chanoprivsneeded(
                    &ctx.matrix.server_info.name,
                    nick,
                    &channel_guard.name,
                ))
                .await?;
            return Ok(());
        }

        // Check +u (no kick / peace mode) - only opers can kick
        if channel_guard.modes.no_kick {
            // Check if user is an IRC operator
            let is_oper = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                user.modes.oper
            } else {
                false
            };

            if !is_oper {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_CHANOPRIVSNEEDED,
                    vec![
                        nick.clone(),
                        channel_guard.name.clone(),
                        "Cannot kick users while channel is in peace mode (+u)".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        // Find target user
        let target_lower = irc_to_lower(target_nick);
        let target_uid = match ctx.matrix.nicks.get(&target_lower) {
            Some(uid) => uid.value().clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHNICK,
                    vec![
                        nick.clone(),
                        target_nick.to_string(),
                        "No such nick".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Check if target is in channel
        if !channel_guard.is_member(&target_uid) {
            ctx.sender
                .send(err_usernotinchannel(
                    &ctx.matrix.server_info.name,
                    nick,
                    target_nick,
                    &channel_guard.name,
                ))
                .await?;
            return Ok(());
        }

        let canonical_name = channel_guard.name.clone();
        let kick_reason = reason
            .map(|s| s.to_string())
            .unwrap_or_else(|| nick.clone());

        // Broadcast KICK to channel (before removing)
        let kick_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, "localhost")),
            command: Command::KICK(
                canonical_name.clone(),
                target_nick.to_string(),
                Some(kick_reason),
            ),
        };

        for uid in channel_guard.members.keys() {
            if let Some(sender) = ctx.matrix.senders.get(uid) {
                let _ = sender.send(kick_msg.clone()).await;
            }
        }

        // Remove target from channel
        channel_guard.remove_member(&target_uid);

        drop(channel_guard);

        // Remove channel from target's list
        if let Some(user) = ctx.matrix.users.get(&target_uid) {
            let mut user = user.write().await;
            user.channels.remove(&channel_lower);
        }

        info!(
            kicker = %nick,
            target = %target_nick,
            channel = %canonical_name,
            "User kicked from channel"
        );

        Ok(())
    }
}
