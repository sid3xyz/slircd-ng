//! PART command handler.

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, server_reply, user_mask_from_state,
};
use crate::handlers::core::traits::TypedContext;
use crate::state::Registered;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

/// Handler for PART command.
pub struct PartHandler;

#[async_trait]
impl PostRegHandler for PartHandler {
    async fn handle(
        &self,
        ctx: &mut TypedContext<'_, Registered>,
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
pub(super) async fn leave_channel_internal(
    ctx: &mut Context<'_>,
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

    let prefix = Prefix::Nickname(nick.to_string(), user_name.to_string(), host.to_string());

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
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NOTONCHANNEL,
                vec![nick.to_string(), channel_lower.to_string(), e],
            );
            ctx.sender.send(reply).await?;
        }
        Err(_) => {
            // Actor dropped
            ctx.matrix.channels.remove(channel_lower);
        }
    }

    Ok(())
}
