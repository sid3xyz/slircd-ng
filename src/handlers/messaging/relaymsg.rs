//! RELAYMSG command handler (Ergo extension).
//!
//! The RELAYMSG command allows relaying messages between IRC networks.
//! This is an Ergo extension for network bridges and bouncers.
//!
//! Format: `RELAYMSG <relay_from> <target> :<text>`
//!
//! Where:
//! - relay_from: The original sender (network/server/nick format)
//! - target: The destination (channel or user)
//! - text: The message content
//!
//! The relayed message appears with a special prefix indicating the relay source.
//! Only IRC operators can use this command (security measure).

use super::super::{Context, HandlerError, HandlerResult, PostRegHandler, server_reply};
use super::common::{SenderSnapshot, RouteOptions, route_to_channel_with_snapshot, route_to_user_with_snapshot};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Command, Message, MessageRef, Prefix, Response, irc_to_lower};
use tracing::debug;

pub struct RelayMsgHandler;

#[async_trait]
impl PostRegHandler for RelayMsgHandler {
    async fn handle(&self, ctx: &mut Context<'_, RegisteredState>, msg: &MessageRef<'_>) -> HandlerResult {
        // Extract RELAYMSG parameters
        // Proto now correctly parses: RELAYMSG <target> <relay_from> <text>
        let relay_from = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let target = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        let text = msg.arg(2).ok_or(HandlerError::NeedMoreParams)?;

        if relay_from.is_empty() || target.is_empty() || text.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        // Validate relay_from nick format FIRST (before oper check)
        // Valid format: "nick/service" (e.g., "smt/discord")
        // Invalid: contains '!' or missing '/' designator
        if relay_from.contains('!') || !relay_from.contains('/') {
            let reply = Message {
                tags: None,
                prefix: Some(Prefix::ServerName(ctx.matrix.server_info.name.clone())),
                command: Command::FAIL(
                    "RELAYMSG".to_string(),
                    "INVALID_NICK".to_string(),
                    vec![format!("Invalid relay nick format: {}", relay_from)],
                ),
            };
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build sender snapshot for routing, but override nick with relay_from
        let mut snapshot = SenderSnapshot::build(ctx)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;
        
        // Override the snapshot nick to be the relay_from so the message appears from that nick
        snapshot.nick = relay_from.to_string();

        // Create the relayed message with relay prefix
        let relay_prefix = Prefix::Nickname(
            relay_from.to_string(),
            "relay".to_string(),
            "relay".to_string(),
        );

        let relayed_msg = Message {
            tags: None,
            prefix: Some(relay_prefix),
            command: slirc_proto::Command::PRIVMSG(target.to_string(), text.to_string()),
        };

        // Determine if target is a channel or user
        if target.is_channel_name() {
            // Channel target
            let target_lower = irc_to_lower(target);

            // Check if channel exists
            if !ctx.matrix.channel_manager.channels.contains_key(&target_lower) {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![ctx.state.nick.clone(), target.to_string(), "No such channel".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            let route_opts = RouteOptions {
                send_away_reply: false,
                status_prefix: None,
            };

            let _ = route_to_channel_with_snapshot(
                ctx,
                &target_lower,
                relayed_msg,
                &route_opts,
                None,
                None,
                &snapshot,
            )
            .await;

            debug!(
                relay_from = %relay_from,
                target = %target,
                "RELAYMSG relayed to channel"
            );
        } else {
            // User target
            let target_lower = irc_to_lower(target);

            // Check if user exists
            if ctx.matrix.user_manager.nicks.get(&target_lower).is_none() {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHNICK,
                    vec![ctx.state.nick.clone(), target.to_string(), "No such nick".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }

            let route_opts = RouteOptions {
                send_away_reply: true,
                status_prefix: None,
            };

            let _ = route_to_user_with_snapshot(
                ctx,
                &target_lower,
                relayed_msg,
                &route_opts,
                None,
                None,
                &snapshot,
            )
            .await;

            debug!(
                relay_from = %relay_from,
                target = %target,
                "RELAYMSG relayed to user"
            );
        }

        Ok(())
    }
}
