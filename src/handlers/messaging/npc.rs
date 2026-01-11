//! NPC command handler (ROLEPLAY extension).
//!
//! The NPC command allows users to send messages to a channel as a different character/nick.
//! This is part of the ROLEPLAY IRCv3 extension from Ergo.
//!
//! Format: `NPC <channel> <nick> :<text>`
//!
//! The message appears in the channel as if sent by the specified nick (roleplay character),
//! but with a special prefix indicating the actual sender.

use super::super::{Context, HandlerError, HandlerResult, PostRegHandler, is_user_in_channel, channel_has_mode, server_reply};
use super::common::{SenderSnapshot, RouteOptions, route_to_channel_with_snapshot};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Message, MessageRef, Prefix, Response, irc_to_lower};
use tracing::debug;

pub struct NpcHandler;

#[async_trait]
impl PostRegHandler for NpcHandler {
    async fn handle(&self, ctx: &mut Context<'_, RegisteredState>, msg: &MessageRef<'_>) -> HandlerResult {
        // Extract NPC parameters: channel, nick, text
        let channel = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let npc_nick = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;

        if channel.is_empty() || npc_nick.is_empty() || text.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        // Must be a valid channel name
        if !channel.is_channel_name() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOSUCHCHANNEL,
                vec![ctx.state.nick.clone(), channel.to_string(), "No such channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let channel_lower = irc_to_lower(channel);

        // Check if channel exists and user is in it
        if !is_user_in_channel(ctx, ctx.uid, &channel_lower).await {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_CANNOTSENDTOCHAN,
                vec![ctx.state.nick.clone(), channel.to_string(), "Cannot send to channel (not a member)".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Check channel mode +E (roleplay enabled) - required for NPC messages
        if !channel_has_mode(ctx, &channel_lower, crate::state::actor::ChannelMode::Roleplay).await
        {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_CANNOTSENDRP,
                vec![ctx.state.nick.clone(), channel.to_string(), "Roleplay mode not enabled (+E)".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build sender snapshot for routing
        let snapshot = SenderSnapshot::build(ctx)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Create the NPC message with special prefix
        // Format: *<npc_nick>*!<real_nick>@npc (asterisks on both sides per Ergo spec)
        let wrapped_npc_nick = format!("*{}*", npc_nick);
        let npc_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                wrapped_npc_nick.clone(),
                snapshot.user.clone(),
                "npc".to_string(),
            )),
            command: slirc_proto::Command::PRIVMSG(channel.to_string(), text.to_string()),
        };

        // Route to channel members
        let route_opts = RouteOptions {
            send_away_reply: false,
            status_prefix: None,
        };

        let _ = route_to_channel_with_snapshot(
            ctx,
            &channel_lower,
            npc_msg,
            &route_opts,
            None,  // timestamp
            None,  // msgid
            &snapshot,
        )
        .await;

        debug!(
            channel = %channel,
            npc_nick = %npc_nick,
            real_nick = %snapshot.nick,
            "NPC message sent"
        );

        Ok(())
    }
}

