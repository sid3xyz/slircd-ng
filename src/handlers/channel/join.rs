//! JOIN command handler and related functionality.

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, server_reply, user_mask_from_state,
    user_prefix, with_label,
};
use crate::state::RegisteredState;
use crate::db::ChannelRepository;
use crate::security::UserContext;
use crate::state::MemberModes;
use async_trait::async_trait;
use slirc_proto::{ChannelExt, Command, Message, MessageRef, Prefix, Response, irc_to_lower};
use std::sync::Arc;
use tracing::info;

/// Handler for JOIN command.
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
                &ctx.matrix.server_info.name,
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
                    &ctx.matrix.server_info.name,
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

/// Join a single channel.
async fn join_channel(
    ctx: &mut Context<'_, RegisteredState>,
    channel_name: &str,
    provided_key: Option<&str>,
) -> HandlerResult {
    let channel_lower = irc_to_lower(channel_name);
    let (nick, user_name, visible_host) = user_mask_from_state(ctx, ctx.uid)
        .await
        .ok_or(HandlerError::NickOrUserMissing)?;

    let (real_host, realname, session_id, account, away_message, caps) = {
        let user_ref = ctx
            .matrix
            .users
            .get(ctx.uid)
            .ok_or(HandlerError::NickOrUserMissing)?;
        let user = user_ref.read().await;
        (
            user.host.clone(),
            user.realname.clone(),
            user.session_id,
            user.account.clone(),
            user.away.clone(),
            user.caps.clone(),
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

    // Check AKICK before joining
    if ctx.matrix.registered_channels.contains(&channel_lower)
        && let Some(akick) = check_akick(ctx, &channel_lower, &nick, &user_name).await
    {
        let reason = akick
            .reason
            .as_deref()
            .unwrap_or("You are banned from this channel");
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
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

    // Check auto modes if registered
    let initial_modes = if ctx.matrix.registered_channels.contains(&channel_lower) {
        check_auto_modes(ctx, &channel_lower).await
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
            Ok(Err(reason)) => {
                if reason == "ERR_CHANNEL_TOMBSTONE" && attempt == 0 {
                    if ctx.matrix.channels.remove(&channel_lower).is_some() {
                        crate::metrics::ACTIVE_CHANNELS.dec();
                    }
                    attempt += 1;
                    continue;
                }

                send_join_error(ctx, &nick, channel_name, &reason).await?;
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

/// Check if user should receive auto-op or auto-voice on a registered channel.
/// Returns Some(MemberModes) if the user has access, None otherwise.
async fn check_auto_modes(ctx: &Context<'_, RegisteredState>, channel_lower: &str) -> Option<MemberModes> {
    let account_name = {
        let user = ctx.matrix.users.get(ctx.uid)?;
        let user = user.read().await;

        if !user.modes.registered {
            return None;
        }

        user.account.clone()?
    };

    let account = ctx.db.accounts().find_by_name(&account_name).await.ok()??;
    let channel_record = ctx.db.channels().find_by_name(channel_lower).await.ok()??;

    if account.id == channel_record.founder_account_id {
        return Some(MemberModes {
            owner: false,
            admin: false,
            op: true,
            halfop: false,
            voice: false,
            join_time: None,
        });
    }

    let access = ctx
        .db
        .channels()
        .get_access(channel_record.id, account.id)
        .await
        .ok()??;

    let op = ChannelRepository::has_op_access(&access.flags);
    let voice = ChannelRepository::has_voice_access(&access.flags);

    if op || voice {
        Some(MemberModes {
            owner: false,
            admin: false,
            op,
            halfop: false,
            voice,
            join_time: None,
        })
    } else {
        None
    }
}

/// Check if user is on the AKICK list for a channel.
/// Returns the matching AKICK entry if found.
async fn check_akick(
    ctx: &Context<'_, RegisteredState>,
    channel_lower: &str,
    nick: &str,
    user: &str,
) -> Option<crate::db::ChannelAkick> {
    let channel_record = ctx.db.channels().find_by_name(channel_lower).await.ok()??;

    let host = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        let user_state = user_ref.read().await;
        user_state.host.clone()
    } else {
        // Post-registration, user should always exist in matrix.users
        ctx.remote_addr.ip().to_string()
    };

    ctx.db
        .channels()
        .check_akick(channel_record.id, nick, user, &host)
        .await
        .ok()?
}

/// Handle successful JOIN - send topic, names, and update user state.
#[allow(clippy::too_many_arguments)]
async fn handle_join_success(
    ctx: &mut Context<'_, RegisteredState>,
    channel_sender: &tokio::sync::mpsc::Sender<crate::state::actor::ChannelEvent>,
    channel_lower: &str,
    nick: &str,
    user_name: &str,
    visible_host: &str,
    extended_join_msg: &Message,
    standard_join_msg: &Message,
    away_message: &Option<String>,
    data: crate::state::actor::JoinSuccessData,
) -> HandlerResult {
    // Add channel to user's list
    if let Some(user) = ctx.matrix.users.get(&ctx.uid.to_string()) {
        let mut user = user.write().await;
        user.channels.insert(channel_lower.to_string());
    }

    // Send JOIN message to user
    let self_join_msg = if ctx.state.capabilities.contains("extended-join") {
        extended_join_msg.clone()
    } else {
        standard_join_msg.clone()
    };
    ctx.sender.send(self_join_msg).await?;

    // Broadcast AWAY if user is away
    if let Some(away_text) = away_message {
        let away_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, visible_host)),
            command: Command::AWAY(Some(away_text.clone())),
        };
        ctx.matrix
            .broadcast_to_channel_with_cap(
                channel_lower,
                away_msg,
                Some(ctx.uid),
                Some("away-notify"),
                None,
            )
            .await;
    }

    // Send topic if exists
    send_channel_topic(ctx, nick, &data).await?;

    // Send names list
    send_names_list(ctx, channel_sender, nick, &data).await?;

    Ok(())
}

/// Send channel topic to user (RPL_TOPIC and RPL_TOPICWHOTIME).
async fn send_channel_topic(
    ctx: &mut Context<'_, RegisteredState>,
    nick: &str,
    data: &crate::state::actor::JoinSuccessData,
) -> HandlerResult {
    if let Some(topic) = &data.topic {
        let topic_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_TOPIC,
            vec![
                nick.to_string(),
                data.channel_name.clone(),
                topic.text.clone(),
            ],
        );
        ctx.sender.send(topic_reply).await?;

        let topic_who_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_TOPICWHOTIME,
            vec![
                nick.to_string(),
                data.channel_name.clone(),
                topic.set_by.clone(),
                topic.set_at.to_string(),
            ],
        );
        ctx.sender.send(topic_who_reply).await?;
    }
    Ok(())
}

/// Send channel names list to user (RPL_NAMREPLY and RPL_ENDOFNAMES).
async fn send_names_list(
    ctx: &mut Context<'_, RegisteredState>,
    channel_sender: &tokio::sync::mpsc::Sender<crate::state::actor::ChannelEvent>,
    nick: &str,
    data: &crate::state::actor::JoinSuccessData,
) -> HandlerResult {
    let (members_tx, members_rx) = tokio::sync::oneshot::channel();
    let _ = channel_sender
        .send(crate::state::actor::ChannelEvent::GetMembers {
            reply_tx: members_tx,
        })
        .await;

    if let Ok(members) = members_rx.await {
        let channel_symbol = if data.is_secret { "@" } else { "=" };
        let mut names_list = Vec::new();

        for (uid, member_modes) in members {
            if let Some(user) = ctx.matrix.users.get(&uid) {
                let user = user.read().await;
                let nick_with_prefix = if let Some(prefix) = member_modes.prefix_char() {
                    format!("{}{}", prefix, user.nick)
                } else {
                    user.nick.clone()
                };
                names_list.push(nick_with_prefix);
            }
        }

        let names_reply = server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_NAMREPLY,
            vec![
                nick.to_string(),
                channel_symbol.to_string(),
                data.channel_name.clone(),
                names_list.join(" "),
            ],
        );
        ctx.sender.send(names_reply).await?;
    }

    let end_names = with_label(
        server_reply(
            &ctx.matrix.server_info.name,
            Response::RPL_ENDOFNAMES,
            vec![
                nick.to_string(),
                data.channel_name.clone(),
                "End of /NAMES list".to_string(),
            ],
        ),
        ctx.label.as_deref(),
    );
    ctx.sender.send(end_names).await?;

    Ok(())
}

/// Send appropriate error response for JOIN failure.
async fn send_join_error(
    ctx: &mut Context<'_, RegisteredState>,
    nick: &str,
    channel_name: &str,
    reason: &str,
) -> HandlerResult {
    let (response, message) = match reason {
        "ERR_BANNEDFROMCHAN" => (Response::ERR_BANNEDFROMCHAN, "Cannot join channel (+b)"),
        "ERR_INVITEONLYCHAN" => (Response::ERR_INVITEONLYCHAN, "Cannot join channel (+i)"),
        "ERR_CHANNELISFULL" => (Response::ERR_CHANNELISFULL, "Cannot join channel (+l)"),
        "ERR_BADCHANNELKEY" => (Response::ERR_BADCHANNELKEY, "Cannot join channel (+k)"),
        "ERR_SESSION_INVALID" => (Response::ERR_UNKNOWNERROR, "Session expired. Please retry."),
        "ERR_CHANNEL_TOMBSTONE" => (
            Response::ERR_UNKNOWNERROR,
            "Channel is restarting. Please retry.",
        ),
        _ => (Response::ERR_UNKNOWNERROR, reason),
    };

    let reply = server_reply(
        &ctx.matrix.server_info.name,
        response,
        vec![
            nick.to_string(),
            channel_name.to_string(),
            message.to_string(),
        ],
    );
    ctx.sender.send(reply).await?;
    Ok(())
}

/// Leave all channels (JOIN 0).
async fn leave_all_channels(ctx: &mut Context<'_, RegisteredState>) -> HandlerResult {
    let (nick, user_name, host) = user_mask_from_state(ctx, ctx.uid)
        .await
        .ok_or(HandlerError::NickOrUserMissing)?;

    let channels: Vec<String> = if let Some(user) = ctx.matrix.users.get(ctx.uid) {
        let user = user.read().await;
        user.channels.iter().cloned().collect()
    } else {
        return Ok(());
    };

    for channel_lower in channels {
        super::part::leave_channel_internal(ctx, &channel_lower, &nick, &user_name, &host, None)
            .await?;
    }

    Ok(())
}
