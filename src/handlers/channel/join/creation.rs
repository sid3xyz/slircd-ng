//! Channel creation logic and initial setup.
//!
//! This module contains the core JOIN channel orchestration logic.
//! Channel creation itself is handled by the ChannelActor; this module
//! manages the handshake: checking access, sending events, and handling responses.

use super::super::super::{Context, HandlerError, HandlerResult, user_prefix};
use super::enforcement::{check_akick, check_auto_modes};
use super::responses::{JoinSuccessContext, handle_join_success, send_join_error};
use crate::error::ChannelError;
use crate::security::UserContext;
use crate::state::{RegisteredState, Topic};
use slirc_proto::ircv3::msgid::generate_msgid;
use slirc_proto::ircv3::server_time::format_server_time;
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
    let (
        nick,
        user_name,
        visible_host,
        real_host,
        realname,
        session_id,
        account,
        away_message,
        caps,
        is_registered,
        is_oper,
        oper_type,
    ) = {
        let user_ref = ctx
            .matrix
            .user_manager
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
            user.modes.oper,
            user.modes.oper_type.clone(),
        )
    };

    let user_context = UserContext::for_registration(crate::security::RegistrationParams {
        hostname: real_host.clone(),
        nickname: nick.clone(),
        username: user_name.clone(),
        realname: realname.clone(),
        server: ctx.server_name().to_string(),
        account: account.clone(),
        is_tls: ctx.state.is_tls,
        is_oper,
        oper_type,
    });

    // Check AKICK before joining (pass pre-fetched host)
    if ctx
        .matrix
        .channel_manager
        .registered_channels
        .contains(&channel_lower)
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
    let initial_modes = if ctx
        .matrix
        .channel_manager
        .registered_channels
        .contains(&channel_lower)
    {
        check_auto_modes(ctx, &channel_lower, is_registered, &account).await
    } else {
        None
    };

    // Build JOIN messages
    let account_name = account.as_deref().unwrap_or("*");
    let make_extended_join_msg = || {
        Message {
            tags: None,
            prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
            command: Command::JOIN(
                channel_name.to_string(),
                Some(account_name.to_string()),
                Some(realname.clone()),
            ),
        }
        .with_tag("msgid", Some(generate_msgid()))
        .with_tag("time", Some(format_server_time()))
    };

    let make_standard_join_msg = || {
        Message {
            tags: None,
            prefix: Some(user_prefix(&nick, &user_name, &visible_host)),
            command: Command::JOIN(channel_name.to_string(), None, None),
        }
        .with_tag("msgid", Some(generate_msgid()))
        .with_tag("time", Some(format_server_time()))
    };

    let matrix = ctx.matrix.clone();
    let mut attempt = 0;
    let is_registered_channel = ctx
        .matrix
        .channel_manager
        .registered_channels
        .contains(&channel_lower);

    // Pre-load saved topic for registered channels (passed to actor at spawn)
    let initial_topic = if is_registered_channel {
        ctx.db
            .channels()
            .find_by_name(&channel_lower)
            .await
            .ok()
            .flatten()
            .filter(|r| r.keeptopic)
            .and_then(|r| match (r.topic_text, r.topic_set_by, r.topic_set_at) {
                (Some(text), Some(set_by), Some(set_at)) => Some(Topic {
                    text,
                    set_by,
                    set_at,
                }),
                _ => None,
            })
    } else {
        None
    };

    let mailbox_capacity = ctx.matrix.config.limits.channel_mailbox_capacity;

    loop {
        let observer = ctx.matrix.channel_manager.observer.clone();
        let channel_sender = ctx
            .matrix
            .channel_manager
            .channels
            .entry(channel_lower.clone())
            .or_insert_with(|| {
                crate::metrics::ACTIVE_CHANNELS.inc();
                crate::state::actor::ChannelActor::spawn_with_capacity(
                    channel_name.to_string(),
                    Arc::downgrade(&matrix),
                    initial_topic.clone(),
                    mailbox_capacity,
                    observer,
                )
            })
            .clone();

        let extended_join_msg = make_extended_join_msg();
        info!(?extended_join_msg, "Created extended join msg");
        let standard_join_msg = make_standard_join_msg();

        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        let sender = ctx
            .matrix
            .user_manager
            .senders
            .get(ctx.uid)
            .map(|s| s.value().clone())
            .ok_or(HandlerError::NickOrUserMissing)?;

        let _ = channel_sender
            .send(crate::state::actor::ChannelEvent::Join {
                params: Box::new(crate::state::actor::JoinParams {
                    uid: ctx.uid.to_string(),
                    nick: nick.clone(),
                    sender,
                    caps: caps.clone(),
                    user_context: user_context.clone(),
                    key: provided_key.map(|s| s.to_string()),
                    initial_modes: initial_modes.clone(),
                    join_msg_extended: extended_join_msg.clone(),
                    join_msg_standard: standard_join_msg.clone(),
                    session_id,
                }),
                reply_tx,
            })
            .await;

        match reply_rx.await {
            Ok(Ok(data)) => {
                handle_join_success(
                    ctx,
                    JoinSuccessContext {
                        channel_sender: &channel_sender,
                        channel_lower: &channel_lower,
                        nick: &nick,
                        user_name: &user_name,
                        visible_host: &visible_host,
                        extended_join_msg: &extended_join_msg,
                        standard_join_msg: &standard_join_msg,
                        away_message: &away_message,
                        data,
                        session_id,
                    },
                )
                .await?;
                break;
            }
            Ok(Err(error)) => {
                if matches!(error, ChannelError::ChannelTombstone) && attempt == 0 {
                    if ctx
                        .matrix
                        .channel_manager
                        .channels
                        .remove(&channel_lower)
                        .is_some()
                    {
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
                    if ctx
                        .matrix
                        .channel_manager
                        .channels
                        .remove(&channel_lower)
                        .is_some()
                    {
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
