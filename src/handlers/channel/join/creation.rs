//! Channel creation logic and initial setup.
//!
//! This module contains the core JOIN channel orchestration logic.
//! Channel creation itself is handled by the ChannelActor; this module
//! manages the handshake: checking access, sending events, and handling responses.

use crate::error::ChannelError;
use crate::security::UserContext;
use crate::state::RegisteredState;
use super::super::super::{Context, HandlerError, HandlerResult, user_prefix};
use super::enforcement::{check_akick, check_auto_modes};
use super::responses::{handle_join_success, send_join_error};
use slirc_proto::{Command, Message, Prefix, irc_to_lower};
use std::sync::Arc;
use tracing::info;

/// Join a single channel.
/// This is the main orchestration function for channel joining.
pub(super) async fn join_channel(
    ctx: &mut Context<'_, RegisteredState>,
    channel_name: &str,
    provided_key: Option<&str>,
) -> HandlerResult {
    let channel_lower = irc_to_lower(channel_name);

    // Single user read to capture all needed fields (eliminates redundant lookup)
    let (nick, user_name, visible_host, real_host, realname, session_id, account, away_message, caps, is_registered) = {
        let user_ref = ctx
            .matrix
            .users
            .get(ctx.uid)
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user = user_ref.read().await;
        (
            user.nick.clone(),
            user.user.clone(),
            user.visible_host.clone(),
            user.host.clone(),
            user.realname.clone(),
            user.session_id,
            user.account.clone(),
            user.away.clone(),
            user.caps.clone(),
            user.modes.registered,
        )
    };

    // For registered connections, use remote_addr directly (WEBIRC already applied at registration)
    let ip_addr = ctx.remote_addr.ip();

    let user_context = UserContext::for_registration(
        ip_addr,
        real_host.clone(),
        nick.clone(),
        user_name.clone(),
        realname.clone(),
        ctx.matrix.server_info.name.clone(),
        account.clone(),
    );

    // Check AKICK before joining (pass pre-fetched host)
    if ctx.matrix.registered_channels.contains(&channel_lower)
        && let Some(akick) = check_akick(ctx, &channel_lower, &nick, &user_name, &real_host).await
    {
        let reason = akick
            .reason
            .as_deref()
            .unwrap_or("You are banned from this channel");
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::new(
                "ChanServ".to_string(),
                "ChanServ".to_string(),
                "services.".to_string(),
            )),
            command: Command::NOTICE(
                nick.clone(),
                format!(
                    "You are not permitted to be on \x02{}\x02: {}",
                    channel_name, reason
                ),
            ),
        };
        ctx.sender.send(notice).await?;
        info!(nick = %nick, channel = %channel_name, mask = %akick.mask, "AKICK triggered");
        return Ok(());
    }

    // Check auto modes if registered (pass pre-fetched user data)
    let initial_modes = if ctx.matrix.registered_channels.contains(&channel_lower) {
        check_auto_modes(ctx, &channel_lower, is_registered, &account).await
    } else {
        None
    };

    // Build JOIN messages
    let account_name = account.as_deref().unwrap_or("*");
    let make_extended_join_msg = || Message {
        tags: None,
        prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
        command: Command::JOIN(
            channel_name.to_string(),
            Some(account_name.to_string()),
            Some(realname.clone()),
        ),
    };
    let make_standard_join_msg = || Message {
        tags: None,
        prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
        command: Command::JOIN(channel_name.to_string(), None, None),
    };

    let matrix = ctx.matrix.clone();
    let mut attempt = 0;

    loop {
        let channel_sender = ctx
            .matrix
            .channels
            .entry(channel_lower.clone())
            .or_insert_with(|| {
                crate::metrics::ACTIVE_CHANNELS.inc();
                crate::state::actor::ChannelActor::spawn(
                    channel_name.to_string(),
                    Arc::downgrade(&matrix),
                )
            })
            .clone();

        let extended_join_msg = make_extended_join_msg();
        let standard_join_msg = make_standard_join_msg();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let sender = ctx
            .matrix
            .senders
            .get(ctx.uid)
            .map(|s| s.clone())
            .ok_or(HandlerError::NickOrUserMissing)?;

        let _ = channel_sender
            .send(crate::state::actor::ChannelEvent::Join {
                uid: ctx.uid.to_string(),
                nick: nick.clone(),
                sender,
                caps: caps.clone(),
                user_context: Box::new(user_context.clone()),
                key: provided_key.map(|s| s.to_string()),
                initial_modes: initial_modes.clone(),
                join_msg_extended: Box::new(extended_join_msg.clone()),
                join_msg_standard: Box::new(standard_join_msg.clone()),
                session_id,
                reply_tx,
            })
            .await;

        match reply_rx.await {
            Ok(Ok(data)) => {
                handle_join_success(
                    ctx,
                    &channel_sender,
                    &channel_lower,
                    &nick,
                    &user_name,
                    &visible_host,
                    &extended_join_msg,
                    &standard_join_msg,
                    &away_message,
                    data,
                )
                .await?;
                break;
            }
            Ok(Err(error)) => {
                if matches!(error, ChannelError::ChannelTombstone) && attempt == 0 {
                    if ctx.matrix.channels.remove(&channel_lower).is_some() {
                        crate::metrics::ACTIVE_CHANNELS.dec();
                    }
                    attempt += 1;
                    continue;
                }

                send_join_error(ctx, &nick, channel_name, error).await?;
                break;
            }
            Err(_) => {
                // Channel actor died between entry() and send() - race with cleanup.
                // Retry once to create a fresh actor.
                if attempt == 0 {
                    if ctx.matrix.channels.remove(&channel_lower).is_some() {
                        crate::metrics::ACTIVE_CHANNELS.dec();
                    }
                    attempt += 1;
                    continue;
                }
                return Err(HandlerError::Internal("Channel actor died".to_string()));
            }
        }
    }

    Ok(())
}
