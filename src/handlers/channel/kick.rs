//! KICK command handler.

use super::super::{Context,
    HandlerError, HandlerResult, PostRegHandler,
    err_nosuchnick, err_nosuchchannel, user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

/// Handler for KICK command.
///
/// Uses capability-based authorization (Innovation 4).
pub struct KickHandler;

#[async_trait]
impl PostRegHandler for KickHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let kicker_nick = &ctx.state.nick;

        // KICK <channel> <nick> [reason]
        let channel_name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let target_nick = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        // RFC2812: default comment is the nickname of the user issuing the KICK
        let reason_str = msg.arg(2).unwrap_or(kicker_nick).to_string();

        if channel_name.is_empty() || target_nick.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;
        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel_tx = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                ctx.sender
                    .send(err_nosuchchannel(&ctx.matrix.server_info.name, &nick, channel_name))
                    .await?;
                return Ok(());
            }
        };

        // Find target user
        let target_lower = irc_to_lower(target_nick);
        let target_uid = match ctx.matrix.nicks.get(&target_lower) {
            Some(uid) => uid.value().clone(),
            None => {
                ctx.sender
                    .send(err_nosuchnick(&ctx.matrix.server_info.name, &nick, target_nick))
                    .await?;
                return Ok(());
            }
        };

        // Request KICK capability from authority (Innovation 4)
        // This pre-authorizes the operation, centralizing permission logic
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let has_kick_cap = authority
            .request_kick_cap(ctx.uid, channel_name)
            .await
            .is_some();

        // If no capability, let actor do the check (maintains backward compat)
        // If capability granted, use force=true to skip actor check
        let (reply_tx, reply_rx) = oneshot::channel();
        let sender_prefix = slirc_proto::Prefix::new(nick.clone(), user, host);

        let event = ChannelEvent::Kick {
            sender_uid: ctx.uid.to_string(),
            sender_prefix,
            target_uid: target_uid.clone(),
            target_nick: target_nick.to_string(),
            reason: reason_str,
            force: has_kick_cap,
            reply_tx,
        };

        if (channel_tx.send(event).await).is_err() {
            return Ok(());
        }

        match reply_rx.await {
            Ok(Ok(())) => {
                // Success.
                // We also need to remove channel from target's user struct.
                if let Some(user) = ctx.matrix.users.get(&target_uid) {
                    let mut user = user.write().await;
                    user.channels.remove(&channel_lower);
                }

                info!(
                    kicker = %nick,
                    target = %target_nick,
                    channel = %channel_name,
                    "User kicked from channel"
                );
            }
            Ok(Err(e)) => {
                let reply = e.to_irc_reply(&ctx.matrix.server_info.name, &nick, channel_name);
                ctx.sender.send(reply).await?;
            }
            Err(_) => {}
        }

        Ok(())
    }
}
