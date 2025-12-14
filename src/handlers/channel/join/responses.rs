//! JOIN response and reply sending logic.

use crate::error::ChannelError;
use crate::state::RegisteredState;
use super::super::super::{Context, HandlerResult, server_reply, user_prefix, with_label};
use slirc_proto::{Command, Message, Response};

/// Handle successful JOIN - send topic, names, and update user state.
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_join_success(
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
pub(super) async fn send_channel_topic(
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
pub(super) async fn send_names_list(
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
pub(super) async fn send_join_error(
    ctx: &mut Context<'_, RegisteredState>,
    nick: &str,
    channel_name: &str,
    error: ChannelError,
) -> HandlerResult {
    let reply = error.to_irc_reply(&ctx.matrix.server_info.name, nick, channel_name);
    ctx.sender.send(reply).await?;
    Ok(())
}
