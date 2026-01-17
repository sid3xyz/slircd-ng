//! NPC command handler (ROLEPLAY extension).
//!
//! The NPC command allows users to send messages to a channel as a different character/nick.
//! This is part of the ROLEPLAY IRCv3 extension from Ergo.
//!
//! Format: `NPC <channel> <nick> :<text>`
//!
//! The message appears in the channel as if sent by the specified nick (roleplay character),
//! but with a special prefix indicating the actual sender.

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, channel_has_mode, is_user_in_channel,
    server_reply,
};
use super::routing::route_to_channel_with_snapshot;
use super::types::{RouteMeta, RouteOptions, SenderSnapshot};
use crate::handlers::helpers::join_message_args;
use crate::history::{MessageEnvelope, StoredMessage};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Message, MessageRef, Prefix, Response, irc_to_lower};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;
use uuid::Uuid;

pub struct NpcHandler;

#[async_trait]
impl PostRegHandler for NpcHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Extract NPC parameters: channel, nick, text
        let channel = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let npc_nick = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;

        // Join remaining args to handle spaces without IRC trailing ':'
        let text = join_message_args(msg, 2).ok_or(HandlerError::NeedMoreParams)?;

        // Must be a valid channel name
        if !channel.is_channel_name() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOSUCHCHANNEL,
                vec![
                    ctx.state.nick.clone(),
                    channel.to_string(),
                    "No such channel".to_string(),
                ],
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
                vec![
                    ctx.state.nick.clone(),
                    channel.to_string(),
                    "Cannot send to channel (not a member)".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Check channel mode +E (roleplay enabled) - required for NPC messages
        if !channel_has_mode(
            ctx,
            &channel_lower,
            crate::state::actor::ChannelMode::Roleplay,
        )
        .await
        {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_CANNOTSENDRP,
                vec![
                    ctx.state.nick.clone(),
                    channel.to_string(),
                    "Roleplay mode not enabled (+E)".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build sender snapshot for routing
        let snapshot = SenderSnapshot::build(ctx)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Create the NPC message with special prefix
        // Format: *<npc_nick>*!<real_user>@npc (asterisks on both sides per Ergo spec)
        let wrapped_npc_nick = format!("*{}*", npc_nick);

        let npc_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                wrapped_npc_nick.clone(),
                snapshot.user.clone(),
                "npc".to_string(),
            )),
            command: slirc_proto::Command::PRIVMSG(channel.to_string(), text.clone()),
        };

        // Route to channel members
        let route_opts = RouteOptions {
            send_away_reply: false,
            status_prefix: None,
        };

        // Match the timestamp/msgid style used by PRIVMSG/NOTICE for consistent history ordering.
        let now = SystemTime::now();
        let duration = now.duration_since(UNIX_EPOCH).unwrap_or_default();
        let millis = duration.as_millis() as i64;
        let nanotime = millis * 1_000_000;

        let dt = chrono::DateTime::<chrono::Utc>::from_timestamp(
            millis / 1000,
            (millis % 1000) as u32 * 1_000_000,
        )
        .unwrap_or_default();
        let timestamp_iso = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let msgid = Uuid::new_v4().to_string();

        let route_result = route_to_channel_with_snapshot(
            ctx,
            &channel_lower,
            npc_msg,
            &route_opts,
            RouteMeta {
                timestamp: Some(timestamp_iso),
                msgid: Some(msgid.clone()),
                override_nick: Some(wrapped_npc_nick.clone()),
                relaymsg_sender_nick: None,
            },
            &snapshot,
        )
        .await;

        if matches!(route_result, crate::state::actor::ChannelRouteResult::Sent) {
            let prefix_str = format!(
                "{}!{}@{}",
                wrapped_npc_nick, snapshot.user, snapshot.visible_host
            );
            let envelope = MessageEnvelope {
                command: "PRIVMSG".to_string(),
                prefix: prefix_str,
                target: channel.to_string(),
                text: text.clone(),
                tags: None,
            };
            let stored_msg = StoredMessage {
                msgid: msgid.clone(),
                target: channel_lower.clone(),
                sender: wrapped_npc_nick.clone(),
                envelope,
                nanotime,
                account: snapshot.account.clone(),
            };

            if let Err(e) = ctx
                .matrix
                .service_manager
                .history
                .store(channel, stored_msg)
                .await
            {
                debug!("Failed to store history: {}", e);
            }
        }

        debug!(
            channel = %channel,
            npc_nick = %npc_nick,
            real_nick = %ctx.state.nick,
            route_result = ?route_result,
            "NPC message routing complete"
        );

        Ok(())
    }
}
