//! INVITE command handler
//!
//! RFC 2812 - Channel invitation

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_chanoprivsneeded, err_notonchannel,
    err_notregistered, server_reply,
};
use async_trait::async_trait;
use slirc_proto::{Command, MessageRef, Response, irc_to_lower};

/// Handler for INVITE command.
///
/// `INVITE nickname channel`
///
/// Invites a user to a channel.
pub struct InviteHandler;

#[async_trait]
impl Handler for InviteHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // INVITE <nickname> <channel>
        let target_nick = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let channel_name = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        let channel_lower = irc_to_lower(channel_name);
        let target_lower = irc_to_lower(target_nick);

        // Check if target exists
        let target_uid = match ctx.matrix.nicks.get(&target_lower) {
            Some(uid) => uid.value().clone(),
            None => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NOSUCHNICK,
                    vec![
                        nick.clone(),
                        target_nick.to_string(),
                        "No such nick/channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Check if channel exists
        if let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) {
            let channel = channel_ref.read().await;

            // Check if user is on channel
            if !channel.is_member(ctx.uid) {
                ctx.sender
                    .send(err_notonchannel(server_name, nick, channel_name))
                    .await?;
                return Ok(());
            }

            // Check if target already on channel
            if channel.is_member(&target_uid) {
                let reply = server_reply(
                    server_name,
                    Response::ERR_USERONCHANNEL,
                    vec![
                        nick.clone(),
                        target_nick.to_string(),
                        channel_name.to_string(),
                        "is already on channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            // If channel is +i, check if user is op
            if channel.modes.invite_only && !channel.is_op(ctx.uid) {
                ctx.sender
                    .send(err_chanoprivsneeded(server_name, nick, channel_name))
                    .await?;
                return Ok(());
            }
        } else {
            // Channel doesn't exist - some servers allow inviting to non-existent channels
            // We'll allow it for now
        }

        // Send INVITE to target
        if let Some(sender) = ctx.matrix.senders.get(&target_uid) {
            let invite_msg = slirc_proto::Message {
                tags: None,
                prefix: Some(slirc_proto::Prefix::Nickname(
                    nick.clone(),
                    ctx.handshake.user.clone().unwrap_or_default(),
                    "localhost".to_string(), // TODO: get actual host
                )),
                command: Command::INVITE(target_nick.to_string(), channel_name.to_string()),
            };
            let _ = sender.send(invite_msg).await;
        }

        // RPL_INVITING (341)
        let reply = server_reply(
            server_name,
            Response::RPL_INVITING,
            vec![
                nick.clone(),
                target_nick.to_string(),
                channel_name.to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
