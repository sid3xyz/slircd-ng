//! JOIN command handler and related functionality.
//!
//! # RFC 2812 §3.2.1 - Join message
//!
//! Used by clients to start listening to a specific channel.
//!
//! **Specification:** [RFC 2812 §3.2.1](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.1)
//!
//! **Compliance:** 7/7 irctest pass
//!
//! ## Syntax
//! ```text
//! JOIN <channels> [<keys>]
//! JOIN 0  ; Leave all channels
//! ```
//!
//! ## Behavior
//! - Creates channel if it doesn't exist
//! - First joiner receives operator status (@)
//! - Validates channel key if +k mode is set
//! - Enforces bans, invite-only, and user limits
//! - Applies AKICK auto-kicks and auto-modes
//! - Persists registered channel state to database
//! - Rate limits joins to prevent abuse

mod creation;
mod enforcement;
mod responses;

use super::super::{Context, HandlerError, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, MessageRef, Response};

use creation::join_channel;

pub struct JoinHandler;

#[async_trait]
impl PostRegHandler for JoinHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // JOIN <channels> [keys]
        let channels_str = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;

        // Handle "JOIN 0" - leave all channels
        if channels_str == "0" {
            return leave_all_channels(ctx).await;
        }

        // Check join rate limit before processing any channels
        let uid_string = ctx.uid.to_string();
        if !ctx.matrix.rate_limiter.check_join_rate(&uid_string) {
            let nick = ctx.state.nick.clone();
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_TOOMANYCHANNELS,
                vec![
                    nick,
                    channels_str.to_string(),
                    "You are joining channels too quickly. Please wait.".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Parse channel list (comma-separated) and optional keys
        let channels: Vec<&str> = channels_str.split(',').collect();
        let keys: Vec<Option<&str>> = if let Some(keys_str) = msg.arg(1) {
            let mut key_list: Vec<Option<&str>> = keys_str
                .split(',')
                .map(|k| {
                    let trimmed = k.trim();
                    if trimmed.is_empty() {
                        None
                    } else {
                        Some(trimmed)
                    }
                })
                .collect();
            key_list.resize(channels.len(), None);
            key_list
        } else {
            vec![None; channels.len()]
        };

        for (i, channel_name) in channels.iter().enumerate() {
            let channel_name = channel_name.trim();
            if channel_name.is_empty() {
                continue;
            }

            if !channel_name.is_channel_name() {
                let reply = server_reply(
                    ctx.server_name(),
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        ctx.state.nick.clone(),
                        channel_name.to_string(),
                        "Invalid channel name".to_string(),
                    ],
                );

                ctx.sender.send(reply).await?;
                continue;
            }

            let key = keys.get(i).and_then(|k| *k);
            join_channel(ctx, channel_name, key).await?;
        }

        Ok(())
    }
}

/// Leave all channels (JOIN 0).
async fn leave_all_channels(ctx: &mut Context<'_, RegisteredState>) -> HandlerResult {
    // Single user read for both mask and channel list
    let (nick, user_name, host, channels): (String, String, String, Vec<String>) = {
        let user_ref = ctx.matrix.users.get(ctx.uid)
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user = user_ref.read().await;
        (
            user.nick.clone(),
            user.user.clone(),
            user.visible_host.clone(),
            user.channels.iter().cloned().collect(),
        )
    };

    for channel_lower in channels {
        super::part::leave_channel_internal(ctx, &channel_lower, &nick, &user_name, &host, None)
            .await?;
    }

    Ok(())
}
