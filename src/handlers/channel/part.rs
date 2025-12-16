//! PART command handler.
//!
//! # RFC 2812 §3.2.2 - Part message
//!
//! Removes a user from a channel.
//!
//! **Specification:** [RFC 2812 §3.2.2](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.2)
//!
//! **Compliance:** 5/5 irctest pass
//!
//! ## Syntax
//! ```text
//! PART <channels> [<reason>]
//! ```
//!
//! ## Behavior
//! - Can part multiple channels (comma-separated)
//! - Optional part message broadcast to channel
//! - User must be in channel to part it
//! - Destroys empty transient channels
//! - Persists state for registered channels

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, server_reply, user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::state::actor::{ChannelEvent, ChannelError};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

pub struct PartHandler;

#[async_trait]
impl PostRegHandler for PartHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

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
pub(super) async fn leave_channel_internal<S>(
    ctx: &mut Context<'_, S>,
    channel_lower: &str,
    nick: &str,
    user_name: &str,
    host: &str,
    reason: Option<&str>,
) -> HandlerResult {
    // Check if channel exists
    let channel_sender = match ctx.matrix.channels.get(channel_lower) {
        Some(c) => c.clone(),
        None => {
            let reply = Response::err_nosuchchannel(nick, channel_lower)
                .with_prefix(ctx.server_prefix());
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("PART", "ERR_NOSUCHCHANNEL");
            return Ok(());
        }
    };

    let prefix = Prefix::new(nick.to_string(), user_name.to_string(), host.to_string());

    let (reply_tx, reply_rx) = oneshot::channel();
    let event = ChannelEvent::Part {
        uid: ctx.uid.to_string(),
        reason: reason.map(|s| s.to_string()),
        prefix,
        reply_tx,
    };

    if (channel_sender.send(event).await).is_err() {
        // Channel actor died, remove it
        ctx.matrix.channels.remove(channel_lower);
        return Ok(());
    }

    match reply_rx.await {
        Ok(Ok(remaining_members)) => {
            // Success
            // Remove channel from user's list
            if let Some(user) = ctx.matrix.users.get(ctx.uid) {
                let mut user = user.write().await;
                user.channels.remove(channel_lower);
            }

            if remaining_members == 0 {
                ctx.matrix.channels.remove(channel_lower);
                crate::metrics::ACTIVE_CHANNELS.dec();
            }

            info!(nick = %nick, channel = %channel_lower, "User left channel");
        }
        Ok(Err(e)) => {
            let reply = match e {
                ChannelError::NotOnChannel => {
                    Response::err_notonchannel(ctx.server_name(), channel_lower)
                }
                _ => server_reply(
                    ctx.server_name(),
                    Response::ERR_NOTONCHANNEL,
                    vec![
                        nick.to_string(),
                        channel_lower.to_string(),
                        e.to_string(),
                    ],
                ),
            };
            ctx.sender.send(reply).await?;
        }
        Err(_) => {
            // Actor dropped
            ctx.matrix.channels.remove(channel_lower);
        }
    }

    Ok(())
}
