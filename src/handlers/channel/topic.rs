//! TOPIC command handler.
//!
//! # RFC 2812 §3.2.4 - Topic message
//!
//! Changes or views channel topic.
//!
//! **Specification:** [RFC 2812 §3.2.4](https://datatracker.ietf.org/doc/html/rfc2812#section-3.2.4)
//!
//! **Compliance:** 6/6 irctest pass
//!
//! ## Syntax
//! ```text
//! TOPIC <channel>           ; Query topic
//! TOPIC <channel> :<topic>  ; Set topic
//! TOPIC <channel> :         ; Clear topic
//! ```
//!
//! ## Behavior
//! - Query: Returns current topic or RPL_NOTOPIC if unset
//! - Set: Requires channel op (+o) if +t mode is set
//! - Broadcasts topic change to all channel members
//! - Persists topic to database for registered channels with keeptopic enabled
//! - Stores TOPIC event in history for event-playback (Innovation 5)
//! - Uses CapabilityAuthority (Innovation 4) for authorization

use super::super::{
    Context, HandlerError, HandlerResult, PostRegHandler, is_user_in_channel, server_reply,
    user_mask_from_state,
};
use crate::history::{MessageEnvelope, StoredMessage};
use crate::state::RegisteredState;
use crate::state::actor::ChannelEvent;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::oneshot;
use tracing::{debug, info, warn};
use uuid::Uuid;

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
        let channel_tx = match ctx.matrix.channel_manager.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                let reply = Response::err_nosuchchannel(nick, channel_name)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("TOPIC", "ERR_NOSUCHCHANNEL", reply).await?;
                return Ok(());
            }
        };

        // Check if user is in channel
        if !is_user_in_channel(ctx, ctx.uid, &channel_lower).await {
            let reply =
                Response::err_notonchannel(nick, channel_name).with_prefix(ctx.server_prefix());
            ctx.send_error("TOPIC", "ERR_NOTONCHANNEL", reply).await?;
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

                let Ok(info) = reply_rx.await else {
                    return Ok(());
                };

                match info.topic {
                    Some(topic) => {
                        let reply = server_reply(
                            ctx.server_name(),
                            Response::RPL_TOPIC,
                            vec![nick.to_string(), info.name.clone(), topic.text.clone()],
                        );
                        ctx.sender.send(reply).await?;

                        let who_reply = server_reply(
                            ctx.server_name(),
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
                            ctx.server_name(),
                            Response::RPL_NOTOPIC,
                            vec![nick.to_string(), info.name, "No topic is set".to_string()],
                        );
                        ctx.sender.send(reply).await?;
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

                // Save prefix string for persistence (before moving into event)
                let set_by_string = sender_prefix.to_string();

                // Generate msgid and timestamp for event-playback (Innovation 5)
                let msgid = Uuid::new_v4().to_string();
                let now = SystemTime::now();
                let timestamp = chrono::Utc::now()
                    .format("%Y-%m-%dT%H:%M:%S%.3fZ")
                    .to_string();
                let nanotime = now
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as i64;

                // Request TOPIC capability from authority (Innovation 4)
                let authority = ctx.authority();
                let topic_cap = authority.request_topic_cap(ctx.uid, channel_name).await;

                let event = ChannelEvent::SetTopic {
                    params: crate::state::actor::TopicParams {
                        sender_uid: ctx.uid.to_string(),
                        sender_prefix,
                        topic: topic_text.to_string(),
                        msgid: msgid.clone(),
                        timestamp,
                        force: false, // Deprecated in favor of cap
                        cap: topic_cap,
                    },
                    reply_tx,
                };

                if (channel_tx.send(event).await).is_err() {
                    return Ok(());
                }

                match reply_rx.await {
                    Ok(Ok(())) => {
                        info!(nick = %nick, channel = %channel_name, "Topic changed");

                        // Store TOPIC event in history for event-playback (Innovation 5)
                        if ctx.matrix.config.history.should_store_event("TOPIC") {
                            let envelope = MessageEnvelope {
                                command: "TOPIC".to_string(),
                                prefix: set_by_string.clone(),
                                target: channel_name.to_string(),
                                text: topic_text.to_string(),
                                tags: None,
                            };

                            let stored_msg = StoredMessage {
                                msgid,
                                target: channel_lower.clone(),
                                sender: nick.clone(),
                                envelope,
                                nanotime,
                                account: ctx.state.account.clone(),
                            };

                            if let Err(e) = ctx
                                .matrix
                                .service_manager
                                .history
                                .store(channel_name, stored_msg)
                                .await
                            {
                                debug!(error = %e, "Failed to store TOPIC in history");
                            }
                        }

                        // Persist topic to database for registered channels with keeptopic
                        if let Some(channel_record) = ctx
                            .db
                            .channels()
                            .find_by_name(&channel_lower)
                            .await
                            .ok()
                            .flatten()
                            && channel_record.keeptopic
                        {
                            let set_at = chrono::Utc::now().timestamp();
                            if let Err(e) = ctx
                                .db
                                .channels()
                                .save_topic(channel_record.id, topic_text, &set_by_string, set_at)
                                .await
                            {
                                warn!(channel = %channel_name, error = %e, "Failed to persist topic");
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        let reply = e.to_irc_reply(ctx.server_name(), &nick, channel_name);
                        ctx.sender.send(reply).await?;
                    }
                    Err(_) => {}
                }
            }
        }

        Ok(())
    }
}
