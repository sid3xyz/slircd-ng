//! JOIN response and reply sending logic.

use super::super::super::{HandlerResult, server_reply, user_prefix, with_label};
use crate::error::ChannelError;
use crate::handlers::ResponseMiddleware;
use crate::state::actor::ChannelEvent;
use crate::state::managers::client::ClientManager;
use slirc_proto::{Command, Message, Prefix, Response};

/// Context for handling successful JOIN responses.
pub(super) struct JoinSuccessContext<'a> {
    pub response_sender: ResponseMiddleware<'a>,
    pub server_name: String,
    pub active_batch_id: Option<String>,
    pub label: Option<String>,
    pub user_manager: &'a crate::state::UserManager,
    pub channel_manager: &'a crate::state::ChannelManager,
    pub client_manager: &'a ClientManager,
    pub config_multiclient: bool,
    pub uid_str: String,
    pub caps: std::collections::HashSet<String>,

    pub channel_sender: &'a tokio::sync::mpsc::Sender<crate::state::actor::ChannelEvent>,
    pub channel_lower: &'a str,
    pub nick: &'a str,
    pub user_name: &'a str,
    pub visible_host: &'a str,
    pub extended_join_msg: &'a Message,
    pub standard_join_msg: &'a Message,
    pub away_message: &'a Option<String>,
    pub data: crate::state::actor::JoinSuccessData,
    pub session_id: uuid::Uuid,
    pub account: Option<String>,
}

/// Handle successful JOIN - send topic, names, and update user state.
pub(super) async fn handle_join_success(
    join_ctx: JoinSuccessContext<'_>,
) -> Result<Option<Message>, crate::error::HandlerError> {
    let JoinSuccessContext {
        response_sender,
        server_name,
        active_batch_id,
        label,
        user_manager,
        channel_manager,
        client_manager,
        config_multiclient,
        uid_str,
        caps,
        channel_sender,
        channel_lower,
        nick,
        user_name,
        visible_host,
        extended_join_msg,
        standard_join_msg,
        away_message,
        data,
        session_id,
        account,
    } = join_ctx;

    // Add channel to user's list with session validation
    let session_valid = if let Some(user) = user_manager.users.get(&uid_str) {
        let mut user = user.write().await;
        if user.session_id == session_id {
            user.channels.insert(channel_lower.to_string());
            true
        } else {
            false
        }
    } else {
        false
    };

    // If session is invalid (user disconnected), send Quit to ChannelActor to clean up ghost
    if !session_valid {
        let quit_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(
                nick.to_string(),
                user_name.to_string(),
                visible_host.to_string(),
            )),
            command: Command::QUIT(Some("Session terminated".to_string())),
        };
        if let Err(e) = channel_sender
            .send(crate::state::actor::ChannelEvent::Quit {
                uid: uid_str.clone(),
                quit_msg,
                reply_tx: None,
            })
            .await
        {
            tracing::warn!(
                uid = %uid_str,
                channel = %channel_lower,
                error = %e,
                "Failed to send Quit event for ghost member cleanup"
            );
        }
        return Ok(None);
    }

    // Track channel membership for bouncer clients
    if config_multiclient && let Some(account) = account.as_ref() {
        let (modes_tx, modes_rx) = tokio::sync::oneshot::channel();
        let _ = channel_sender
            .send(ChannelEvent::GetMemberModes {
                uid: uid_str.clone(),
                reply_tx: modes_tx,
            })
            .await;
        let member_modes = modes_rx.await.ok().flatten();
        client_manager
            .record_channel_join(account, channel_lower, member_modes.as_ref())
            .await;
    }

    // Send JOIN message to user
    let mut self_join_msg = if caps.contains("extended-join") {
        extended_join_msg.clone()
    } else {
        standard_join_msg.clone()
    };

    // Add batch tag if we're in a batch
    if let Some(ref batch_id) = active_batch_id {
        self_join_msg = self_join_msg.with_tag("batch", Some(batch_id));
    }

    response_sender
        .send(self_join_msg.clone())
        .await
        .map_err(|_| crate::error::HandlerError::Internal("Failed to send JOIN response".into()))?;

    // Broadcast AWAY if user is away
    if let Some(away_text) = away_message {
        let away_msg = Message {
            tags: None,
            prefix: Some(user_prefix(nick, user_name, visible_host)),
            command: Command::AWAY(Some(away_text.clone())),
        };
        channel_manager
            .broadcast_to_channel_with_cap(
                channel_lower,
                away_msg,
                Some(&uid_str),
                Some("away-notify"),
                None,
            )
            .await;
    }

    // Send topic if exists
    send_channel_topic(
        response_sender.clone(),
        &server_name,
        active_batch_id.as_deref(),
        nick,
        &data,
    )
    .await?;

    // Send names list
    send_names_list(
        response_sender.clone(),
        &server_name,
        active_batch_id.as_deref(),
        label.as_deref(),
        user_manager,
        channel_sender,
        nick,
        &data,
    )
    .await?;

    Ok(Some(self_join_msg))
}

/// Send channel topic to user (RPL_TOPIC and RPL_TOPICWHOTIME).
pub(super) async fn send_channel_topic(
    sender: ResponseMiddleware<'_>,
    server_name: &str,
    active_batch_id: Option<&str>,
    nick: &str,
    data: &crate::state::actor::JoinSuccessData,
) -> HandlerResult {
    if let Some(topic) = &data.topic {
        let mut topic_reply = server_reply(
            server_name,
            Response::RPL_TOPIC,
            vec![
                nick.to_string(),
                data.channel_name.clone(),
                topic.text.clone(),
            ],
        );

        // Add batch tag if we're in a batch
        if let Some(batch_id) = active_batch_id {
            topic_reply = topic_reply.with_tag("batch", Some(batch_id));
        }

        sender
            .send(topic_reply)
            .await
            .map_err(|_| crate::error::HandlerError::Internal("Failed to send TOPIC".into()))?;

        let topic_who_reply = server_reply(
            server_name,
            Response::RPL_TOPICWHOTIME,
            vec![
                nick.to_string(),
                data.channel_name.clone(),
                topic.set_by.clone(),
                topic.set_at.to_string(),
            ],
        );
        sender.send(topic_who_reply).await.map_err(|_| {
            crate::error::HandlerError::Internal("Failed to send TOPICWHOTIME".into())
        })?;
    }
    Ok(())
}

/// Send channel names list to user (RPL_NAMREPLY and RPL_ENDOFNAMES).
#[allow(clippy::too_many_arguments)]
pub(super) async fn send_names_list(
    sender: ResponseMiddleware<'_>,
    server_name: &str,
    active_batch_id: Option<&str>,
    label: Option<&str>,
    user_manager: &crate::state::UserManager,
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
        let mut names_list = Vec::with_capacity(members.len());

        for (uid, member_modes) in members {
            if let Some(user) = user_manager.users.get(&uid) {
                let user = user.read().await;
                let nick_with_prefix = if let Some(prefix) = member_modes.prefix_char() {
                    format!("{}{}", prefix, user.nick)
                } else {
                    user.nick.clone()
                };
                names_list.push(nick_with_prefix);
            }
        }

        let mut names_reply = server_reply(
            server_name,
            Response::RPL_NAMREPLY,
            vec![
                nick.to_string(),
                channel_symbol.to_string(),
                data.channel_name.clone(),
                names_list.join(" "),
            ],
        );

        // Add batch tag if we're in a batch
        if let Some(batch_id) = active_batch_id {
            names_reply = names_reply.with_tag("batch", Some(batch_id));
        }

        sender
            .send(names_reply)
            .await
            .map_err(|_| crate::error::HandlerError::Internal("Failed to send NAMREPLY".into()))?;
    }

    let mut end_names = with_label(
        server_reply(
            server_name,
            Response::RPL_ENDOFNAMES,
            vec![
                nick.to_string(),
                data.channel_name.clone(),
                "End of /NAMES list".to_string(),
            ],
        ),
        label,
    );

    // Add batch tag if we're in a batch
    if let Some(batch_id) = active_batch_id {
        end_names = end_names.with_tag("batch", Some(batch_id));
    }

    sender
        .send(end_names)
        .await
        .map_err(|_| crate::error::HandlerError::Internal("Failed to send ENDOFNAMES".into()))?;

    Ok(())
}

/// Send appropriate error response for JOIN failure.
pub(super) async fn send_join_error(
    sender: ResponseMiddleware<'_>,
    server_name: &str,
    nick: &str,
    channel_name: &str,
    error: ChannelError,
) -> HandlerResult {
    let reply = error.to_irc_reply(server_name, nick, channel_name);
    sender
        .send(reply)
        .await
        .map_err(|_| crate::error::HandlerError::Internal("Failed to send error".into()))?;
    Ok(())
}
