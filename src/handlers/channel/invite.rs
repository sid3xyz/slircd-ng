//! INVITE command handler
//!
//! RFC 2812 - Channel invitation

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_chanoprivsneeded, err_notonchannel,
    err_notregistered, server_reply, user_mask_from_state,
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

        // INVITE <nickname> <channel> or INVITE <channel> <nickname>
        // Detect which argument is which based on whether it starts with a channel prefix
        let arg0 = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let arg1 = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        // Track if channel was first (non-standard order) for echo
        let channel_first = arg0.starts_with('#')
            || arg0.starts_with('&')
            || arg0.starts_with('+')
            || arg0.starts_with('!');

        let (target_nick, channel_name) = if channel_first {
            // INVITE #channel nickname format
            (arg1, arg0)
        } else {
            // INVITE nickname #channel format (standard)
            (arg0, arg1)
        };

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

            // If channel is +V (no invite), block invite
            if channel.modes.no_invite {
                let reply = server_reply(
                    server_name,
                    Response::ERR_CHANOPRIVSNEEDED,
                    vec![
                        nick.clone(),
                        channel_name.to_string(),
                        "Invites are disabled on this channel (+V)".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            // If channel is +i but not +g, check if user is op
            if channel.modes.invite_only && !channel.modes.free_invite && !channel.is_op(ctx.uid) {
                ctx.sender
                    .send(err_chanoprivsneeded(server_name, nick, channel_name))
                    .await?;
                return Ok(());
            }

            drop(channel);

            // Add target to channel's invite list
            if let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) {
                let mut channel = channel_ref.write().await;
                channel.invites.insert(target_uid.clone());
            }
        } else {
            // Channel doesn't exist - some servers allow inviting to non-existent channels
            // We'll allow it for now
        }

        // Build the INVITE message - preserve the original argument order
        let (_, _, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        let invite_msg = slirc_proto::Message {
            tags: None,
            prefix: Some(slirc_proto::Prefix::Nickname(
                nick.clone(),
                ctx.handshake.user.clone().unwrap_or_default(),
                host,
            )),
            command: if channel_first {
                Command::INVITE(channel_name.to_string(), target_nick.to_string())
            } else {
                Command::INVITE(target_nick.to_string(), channel_name.to_string())
            },
        };

        // Send INVITE to target user
        if let Some(sender) = ctx.matrix.senders.get(&target_uid) {
            let _ = sender.send(invite_msg.clone()).await;
        }

        // IRCv3 invite-notify: broadcast INVITE to channel members who have the capability
        // Exclude the target user (they already received the direct INVITE)
        ctx.matrix
            .broadcast_to_channel_with_cap(
                &channel_lower,
                invite_msg,
                Some(&target_uid),
                Some("invite-notify"),
                None, // No fallback - clients without cap receive nothing
            )
            .await;

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
