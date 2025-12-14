//! KICK command handler.

use super::super::{Context,
    HandlerError, HandlerResult, PostRegHandler,
    user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

/// Handler for KICK command.
///
/// Uses capability-based authorization (Innovation 4).
/// # RFC 2812 §3.2.8
///
/// Kick command - Requests forced removal of a user from a channel.
///
/// **Specification:** [RFC 2812 §3.2.8](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.8)
///
/// **Compliance:** 5/7 irctest pass
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

        // KICK <channel[,channel2,...]> <nick[,nick2,...]> [reason]
        let channels_arg = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let targets_arg = msg.arg(1).ok_or(HandlerError::NeedMoreParams)?;
        // RFC2812: default comment is the nickname of the user issuing the KICK
        let reason_str = msg.arg(2).unwrap_or(kicker_nick).to_string();

        if channels_arg.is_empty() || targets_arg.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
            .await
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Split comma-separated channels and targets
        let channel_names: Vec<&str> = channels_arg.split(',').map(|s| s.trim()).collect();
        let target_nicks: Vec<&str> = targets_arg.split(',').map(|s| s.trim()).collect();

        // RFC 2812: If multiple channels/users, they must be paired 1:1
        // Modern: Most servers only support 1 channel with multiple nicks
        // We'll support both: if N channels, pair with N nicks (or repeat last channel)
        let pairs: Vec<(&str, &str)> = if channel_names.len() == 1 {
            // Single channel, multiple nicks
            target_nicks.iter().map(|&nick| (channel_names[0], nick)).collect()
        } else if channel_names.len() == target_nicks.len() {
            // Equal counts: pair them 1:1
            channel_names.into_iter().zip(target_nicks).collect()
        } else {
            // Mismatch: pair as many as possible, ignore extras
            channel_names.into_iter().zip(target_nicks).collect()
        };

        for (channel_name, target_nick) in pairs {
            if channel_name.is_empty() || target_nick.is_empty() {
                continue;
            }

            let channel_lower = irc_to_lower(channel_name);

            // Get channel
            let channel_tx = match ctx.matrix.channels.get(&channel_lower) {
                Some(c) => c.clone(),
                None => {
                    let reply = Response::err_nosuchchannel(&nick, channel_name)
                        .with_prefix(Prefix::ServerName(ctx.matrix.server_info.name.clone()));
                    ctx.sender.send(reply).await?;
                    crate::metrics::record_command_error("KICK", "ERR_NOSUCHCHANNEL");
                    continue;
                }
            };

            // Find target user
            let target_lower = irc_to_lower(target_nick);
            let target_uid = match ctx.matrix.nicks.get(&target_lower) {
                Some(uid) => uid.value().clone(),
                None => {
                    let reply = Response::err_nosuchnick(&nick, target_nick)
                        .with_prefix(Prefix::ServerName(ctx.matrix.server_info.name.clone()));
                    ctx.sender.send(reply).await?;
                    crate::metrics::record_command_error("KICK", "ERR_NOSUCHNICK");
                    continue;
                }
            };

            // Request KICK capability from authority (Innovation 4)
            let authority = CapabilityAuthority::new(ctx.matrix.clone());
            let has_kick_cap = authority
                .request_kick_cap(ctx.uid, channel_name)
                .await
                .is_some();

            // If no capability, let actor do the check (maintains backward compat)
            // If capability granted, use force=true to skip actor check
            let (reply_tx, reply_rx) = oneshot::channel();
            let sender_prefix = slirc_proto::Prefix::new(nick.clone(), user.clone(), host.clone());

            let event = ChannelEvent::Kick {
                sender_uid: ctx.uid.to_string(),
                sender_prefix,
                target_uid: target_uid.clone(),
                target_nick: target_nick.to_string(),
                reason: reason_str.clone(),
                force: has_kick_cap,
                reply_tx,
            };

            if (channel_tx.send(event).await).is_err() {
                continue;
            }

            match reply_rx.await {
                Ok(Ok(())) => {
                    // Success.
                    // We also need to remove channel from target's user struct.
                    if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
                        let mut user_data = user_ref.write().await;
                        user_data.channels.remove(&channel_lower);
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
        }

        Ok(())
    }
}
