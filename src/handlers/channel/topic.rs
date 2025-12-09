//! TOPIC command handler.
//!
//! Uses CapabilityAuthority (Innovation 4) for centralized authorization.

use super::super::{Context,
    HandlerError, HandlerResult, PostRegHandler,
    is_user_in_channel, server_reply, user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response, irc_to_lower};
use tokio::sync::oneshot;
use tracing::info;

/// Handler for TOPIC command.
pub struct TopicHandler;

#[async_trait]
impl PostRegHandler for TopicHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let (nick, _user_name) = ctx.nick_user();

        // TOPIC <channel> [new_topic]
        let channel_name = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        let new_topic = msg.arg(1);

        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel_tx = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = Response::err_nosuchchannel(nick, channel_name)
                    .with_prefix(Prefix::ServerName(ctx.matrix.server_info.name.clone()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("TOPIC", "ERR_NOSUCHCHANNEL");
                return Ok(());
            }
        };

        // Check if user is in channel
        if !is_user_in_channel(ctx, ctx.uid, &channel_lower).await {
            let reply = Response::err_notonchannel(nick, channel_name)
                .with_prefix(Prefix::ServerName(ctx.matrix.server_info.name.clone()));
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("TOPIC", "ERR_NOTONCHANNEL");
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
                let sender_prefix = slirc_proto::Prefix::new(nick.clone(), user, host);

                // Request TOPIC capability from authority (Innovation 4)
                let authority = CapabilityAuthority::new(ctx.matrix.clone());
                let has_topic_cap = authority
                    .request_topic_cap(ctx.uid, channel_name)
                    .await
                    .is_some();

                let event = ChannelEvent::SetTopic {
                    sender_uid: ctx.uid.to_string(),
                    sender_prefix,
                    topic: topic_text.to_string(),
                    force: has_topic_cap,
                    reply_tx,
                };

                if (channel_tx.send(event).await).is_err() {
                    return Ok(());
                }

                match reply_rx.await {
                    Ok(Ok(())) => {
                        info!(nick = %nick, channel = %channel_name, "Topic changed");
                    }
                    Ok(Err(e)) => {
                        let reply = e.to_irc_reply(
                            &ctx.matrix.server_info.name,
                            &nick,
                            channel_name,
                        );
                        ctx.sender.send(reply).await?;
                    }
                    Err(_) => {}
                }
            }
        }

        Ok(())
    }
}
