//! TOPIC command handler.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_chanoprivsneeded, err_notonchannel,
    require_registered, server_reply, user_mask_from_state,
};
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

/// Handler for TOPIC command.
pub struct TopicHandler;

#[async_trait]
impl Handler for TopicHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let (nick, _user_name) = require_registered(ctx)?;

        // TOPIC <channel> [new_topic]
        let channel_name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let new_topic = msg.arg(1);

        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel_tx = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NOSUCHCHANNEL,
                    vec![
                        nick.to_string(),
                        channel_name.to_string(),
                        "No such channel".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Check if user is in channel
        let user_in_channel = if let Some(user) = ctx.matrix.users.get(ctx.uid) {
            let user = user.read().await;
            user.channels.contains(&channel_lower)
        } else {
            false
        };

        if !user_in_channel {
            ctx.sender
                .send(err_notonchannel(
                    &ctx.matrix.server_info.name,
                    nick,
                    channel_name,
                ))
                .await?;
            return Ok(());
        }

        match new_topic {
            None => {
                // Query topic
                let (reply_tx, reply_rx) = oneshot::channel();
                let event = ChannelEvent::GetInfo {
                    requester_uid: Some(ctx.uid.to_string()),
                    reply_tx,
                };

                if (channel_tx.send(event).await).is_err() {
                    return Ok(());
                }

                if let Ok(info) = reply_rx.await {
                    match info.topic {
                        Some(topic) => {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::RPL_TOPIC,
                                vec![nick.to_string(), info.name.clone(), topic.text.clone()],
                            );
                            ctx.sender.send(reply).await?;

                            let who_reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::RPL_TOPICWHOTIME,
                                vec![
                                    nick.to_string(),
                                    info.name,
                                    topic.set_by.clone(),
                                    topic.set_at.to_string(),
                                ],
                            );
                            ctx.sender.send(who_reply).await?;
                        }
                        None => {
                            let reply = server_reply(
                                &ctx.matrix.server_info.name,
                                Response::RPL_NOTOPIC,
                                vec![nick.to_string(), info.name, "No topic is set".to_string()],
                            );
                            ctx.sender.send(reply).await?;
                        }
                    }
                }
            }
            Some(topic_text) => {
                // Set topic
                let (reply_tx, reply_rx) = oneshot::channel();
                let (nick, user, host) = user_mask_from_state(ctx, ctx.uid)
                    .await
                    .ok_or(HandlerError::NickOrUserMissing)?;
                let sender_prefix = slirc_proto::Prefix::Nickname(nick.clone(), user, host);

                let event = ChannelEvent::SetTopic {
                    sender_uid: ctx.uid.to_string(),
                    sender_prefix,
                    topic: topic_text.to_string(),
                    force: false,
                    reply_tx,
                };

                if (channel_tx.send(event).await).is_err() {
                    return Ok(());
                }

                match reply_rx.await {
                    Ok(Ok(())) => {
                        info!(nick = %nick, channel = %channel_name, "Topic changed");
                    }
                    Ok(Err(err_code)) => {
                        if err_code == "ERR_CHANOPRIVSNEEDED" {
                            let reply = err_chanoprivsneeded(
                                &ctx.matrix.server_info.name,
                                &nick,
                                channel_name,
                            );
                            ctx.sender.send(reply).await?;
                        }
                    }
                    Err(_) => {}
                }
            }
        }

        Ok(())
    }
}
