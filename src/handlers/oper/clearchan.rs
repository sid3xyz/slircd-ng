//! CLEARCHAN command handler for operators.
//!
//! Allows operators to reset channel states.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelEvent, ClearTarget};
use crate::{require_arg_or_reply, require_oper_cap};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response};
use tokio::sync::oneshot;

/// Handler for CLEARCHAN command.
///
/// `CLEARCHAN <channel> <MODES|BANS|OPS|VOICES>`
pub struct ClearchanHandler;

#[async_trait]
impl PostRegHandler for ClearchanHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Request oper capability
        // Use a generic channel management cap if specific one doesn't exist
        let Some(_cap) = require_oper_cap!(ctx, "CLEARCHAN", request_clearchan_cap) else {
            return Ok(());
        };

        let Some(channel_name) = require_arg_or_reply!(ctx, msg, 0, "CLEARCHAN") else {
            return Ok(());
        };
        let Some(target_type) = require_arg_or_reply!(ctx, msg, 1, "CLEARCHAN") else {
            return Ok(());
        };

        let clear_target = match target_type.to_uppercase().as_str() {
            "MODES" => ClearTarget::Modes,
            "BANS" => ClearTarget::Bans,
            "OPS" => ClearTarget::Ops,
            "VOICES" => ClearTarget::Voices,
            _ => {
                ctx.send_reply(
                    Response::ERR_UNKNOWNERROR,
                    vec![
                        ctx.nick().to_string(),
                        channel_name.to_string(),
                        format!(
                            "Invalid clear target: {}. Use MODES, BANS, OPS, or VOICES.",
                            target_type
                        ),
                    ],
                )
                .await?;
                return Ok(());
            }
        };

        let channel_sender =
            if let Some(sender) = ctx.matrix.channel_manager.channels.get(channel_name) {
                sender.value().clone()
            } else {
                ctx.send_reply(
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        ctx.nick().to_string(),
                        channel_name.to_string(),
                        "No such channel".to_string(),
                    ],
                )
                .await?;
                return Ok(());
            };

        let (nick, user, host) = match crate::handlers::user_mask_from_state(ctx, ctx.uid).await {
            Some(mask) => mask,
            None => {
                ctx.send_reply(
                    Response::ERR_UNKNOWNERROR,
                    vec![
                        ctx.nick().to_string(),
                        channel_name.to_string(),
                        "User vanished".to_string(),
                    ],
                )
                .await?;
                return Ok(());
            }
        };

        let (tx, rx) = oneshot::channel();
        let event = ChannelEvent::Clear {
            sender_uid: ctx.uid.to_string(),
            sender_prefix: Prefix::new(nick, user, host),
            target: clear_target,
            reply_tx: tx,
        };

        if channel_sender.send(event).await.is_err() {
            ctx.send_reply(
                Response::ERR_UNKNOWNERROR,
                vec![
                    ctx.nick().to_string(),
                    channel_name.to_string(),
                    "Channel actor is dead".to_string(),
                ],
            )
            .await?;
            return Ok(());
        }

        match rx.await {
            Ok(Ok(())) => {
                // Success notice is broadcast by the actor itself
                Ok(())
            }
            Ok(Err(err)) => {
                let msg = err.to_irc_reply(ctx.server_name(), ctx.nick(), channel_name);
                ctx.sender.send(msg).await?;
                Ok(())
            }
            Err(_) => {
                ctx.send_reply(
                    Response::ERR_UNKNOWNERROR,
                    vec![
                        ctx.nick().to_string(),
                        channel_name.to_string(),
                        "Channel actor timeout".to_string(),
                    ],
                )
                .await?;
                Ok(())
            }
        }
    }
}
