//! TOPIC command handler.

use super::super::{
    Context, Handler, HandlerError, HandlerResult, err_notonchannel, require_registered,
    server_reply, user_mask_from_state, user_prefix,
};
use crate::state::Topic;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Response, irc_to_lower};
use tracing::info;

/// Handler for TOPIC command.
pub struct TopicHandler;

#[async_trait]
impl Handler for TopicHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let (nick, user_name) = require_registered(ctx)?;

        // TOPIC <channel> [new_topic]
        let channel_name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let new_topic = msg.arg(1);

        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel = match ctx.matrix.channels.get(&channel_lower) {
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

        let mut channel_guard = channel.write().await;

        // Check if user is in channel
        if !channel_guard.is_member(ctx.uid) {
            ctx.sender
                .send(err_notonchannel(
                    &ctx.matrix.server_info.name,
                    nick,
                    &channel_guard.name,
                ))
                .await?;
            return Ok(());
        }

        let canonical_name = channel_guard.name.clone();

        match new_topic {
            None => {
                // Query topic
                match &channel_guard.topic {
                    Some(topic) => {
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_TOPIC,
                            vec![nick.to_string(), canonical_name.clone(), topic.text.clone()],
                        );
                        ctx.sender.send(reply).await?;

                        let who_reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::RPL_TOPICWHOTIME,
                            vec![
                                nick.to_string(),
                                canonical_name,
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
                            vec![
                                nick.to_string(),
                                canonical_name,
                                "No topic is set".to_string(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                    }
                }
            }
            Some(topic_text) => {
                // Check +t mode: if set, only ops can change topic
                if channel_guard.modes.topic_lock {
                    // Check if user has op or higher
                    let member_modes = channel_guard.members.get(ctx.uid);
                    let has_op = member_modes.is_some_and(|m| m.has_op_or_higher());

                    if !has_op {
                        let reply = server_reply(
                            &ctx.matrix.server_info.name,
                            Response::ERR_CHANOPRIVSNEEDED,
                            vec![
                                nick.to_string(),
                                canonical_name,
                                "You're not channel operator".to_string(),
                            ],
                        );
                        ctx.sender.send(reply).await?;
                        return Ok(());
                    }
                }

                let (_, _, host) = user_mask_from_state(ctx, ctx.uid)
                    .await
                    .ok_or(HandlerError::NickOrUserMissing)?;

                let new_topic = Topic {
                    text: topic_text.to_string(),
                    set_by: format!("{}!{}@{}", nick, user_name, host),
                    set_at: chrono::Utc::now().timestamp(),
                };
                channel_guard.topic = Some(new_topic);

                // Broadcast topic change to channel
                let topic_msg = Message {
                    tags: None,
                    prefix: Some(user_prefix(nick, user_name, &host)),
                    command: Command::TOPIC(canonical_name.clone(), Some(topic_text.to_string())),
                };

                for uid in channel_guard.members.keys() {
                    if let Some(sender) = ctx.matrix.senders.get(uid) {
                        let _ = sender.send(topic_msg.clone()).await;
                    }
                }

                info!(nick = %nick, channel = %canonical_name, "Topic changed");
            }
        }

        Ok(())
    }
}
