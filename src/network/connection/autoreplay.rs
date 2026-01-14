//! Autoreplay logic for bouncer reattachment.
//!
//! Handles replaying channel JOINs and missed message history when a client
//! reattaches to an existing session (always-on).

use crate::error::HandlerResult;
use crate::handlers::server_reply;
use crate::history::HistoryQuery;
use crate::network::connection::context::ConnectionContext;
use crate::state::actor::ChannelEvent;
use crate::state::{ReattachInfo, RegisteredState};
use chrono::{DateTime, Utc};
use slirc_proto::{Command, Message, Prefix, Response, Tag, irc_to_lower};
use tokio::sync::oneshot;
use tracing::{debug, warn};

/// Perform autoreplay for a reattached session.
///
/// 1. Sends JOIN messages for all active channels.
/// 2. Replays missed history for each channel since `last_seen`.
pub async fn perform_autoreplay(
    ctx: &mut ConnectionContext<'_>,
    reg_state: &RegisteredState,
    info: ReattachInfo,
) -> HandlerResult {
    let nick = &reg_state.nick;
    let server_name = ctx.matrix.server_info.name.clone();

    // We get the Arc<RwLock<User>> and then acquire generic read lock
    let user_arc = ctx
        .matrix
        .user_manager
        .users
        .get(ctx.uid)
        .map(|r| r.value().clone());

    let user_host = if let Some(ua) = user_arc {
        let u = ua.read().await;
        u.visible_host.clone()
    } else {
        warn!(uid = %ctx.uid, "User not found during autoreplay");
        return Ok(());
    };

    let prefix = Prefix::Nickname(nick.clone(), reg_state.user.clone(), user_host);

    debug!(
        uid = %ctx.uid,
        channels = %info.channels.len(),
        replay_since = ?info.replay_since,
        "Starting autoreplay"
    );

    // 1. Rejoin channels
    for (channel_name, _membership) in &info.channels {
        let mut display_name = channel_name.clone();
        let mut topic_snapshot = None;

        // Query channel actor for canonical casing/topic metadata
        let channel_key = irc_to_lower(channel_name);
        if let Some(actor_entry) = ctx.matrix.channel_manager.channels.get(&channel_key) {
            let actor_tx = actor_entry.value().clone();
            let (tx, rx) = oneshot::channel();
            let event = ChannelEvent::GetInfo {
                requester_uid: Some(ctx.uid.to_string()),
                reply_tx: tx,
            };

            if actor_tx.send(event).await.is_ok()
                && let Ok(info) = rx.await
            {
                if !info.name.is_empty() {
                    display_name = info.name.clone();
                }
                topic_snapshot = info.topic;
            }
        }

        // Send JOIN to client using canonical casing when available
        let join = Message::from(Command::JOIN(display_name.clone(), None, None))
            .with_prefix(prefix.clone());

        if let Err(e) = ctx.transport.write_message(&join).await {
            warn!(uid = %ctx.uid, error = ?e, "Failed to send autoreplay JOIN");
            // If we can't write, connection is likely dead, stop replay
            return Ok(());
        }

        // Send channel topic if we captured it
        if let Some(topic) = topic_snapshot {
            let topic_msg = server_reply(
                &server_name,
                Response::RPL_TOPIC,
                vec![nick.clone(), display_name.clone(), topic.text.clone()],
            );
            let _ = ctx.transport.write_message(&topic_msg).await;

            let topic_whotime = server_reply(
                &server_name,
                Response::RPL_TOPICWHOTIME,
                vec![
                    nick.clone(),
                    display_name.clone(),
                    topic.set_by.clone(),
                    topic.set_at.to_string(), // i64 timestamp
                ],
            );
            let _ = ctx.transport.write_message(&topic_whotime).await;
        }
    }

    // 2. Replay history
    for (channel_name, _membership) in &info.channels {
        // Determine start bound per-target: Use device last_seen from reattach info
        // NOTE: When a read-marker manager exists, use it for per-target replay bounds.
        let start_dt_opt = info.replay_since;

        if let Some(start_dt) = start_dt_opt {
            replay_channel_history(ctx, channel_name, start_dt, reg_state).await?;
            // NOTE: When a read-marker manager exists, update it after replay.
        }
    }

    Ok(())
}

async fn replay_channel_history(
    ctx: &mut ConnectionContext<'_>,
    target: &str,
    since: DateTime<Utc>,
    reg_state: &RegisteredState,
) -> Result<(), crate::error::HandlerError> {
    let start_ts = since
        .timestamp()
        .saturating_mul(1_000_000_000)
        .saturating_add(i64::from(since.timestamp_subsec_nanos()));
    let server_name = ctx.matrix.server_info.name.clone();

    let query = HistoryQuery {
        target: target.to_string(),
        start: Some(start_ts),
        end: None,
        limit: 1000,    // Reasonable limit for catch-up
        reverse: false, // Oldest first
    };

    // Correctly access history via service_manager
    match ctx.matrix.service_manager.history.query(query).await {
        Ok(messages) if !messages.is_empty() => {
            // Send a simple CHATHISTORY batch for autoreplay
            // We don't use the full send_history_batch because we don't have a complete Context
            let batch_id = format!("chathistory-{}", uuid::Uuid::new_v4().simple());
            let has_event_playback = reg_state.capabilities.contains("draft/event-playback");

            // Start BATCH
            let batch_start = Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name.clone())),
                command: Command::BATCH(
                    format!("+{}", batch_id),
                    Some(slirc_proto::BatchSubCommand::CUSTOM(
                        "chathistory".to_string(),
                    )),
                    Some(vec![target.to_string()]),
                ),
            };
            let _ = ctx.transport.write_message(&batch_start).await;

            // Send each message with batch tag
            for msg in messages {
                // Filter based on capabilities (same logic as send_history_batch)
                let command_type = msg.envelope.command.as_str();
                match command_type {
                    "PRIVMSG" | "NOTICE" => {
                        // Always include messages
                    }
                    "TOPIC" | "TAGMSG" => {
                        if !has_event_playback {
                            continue;
                        }
                    }
                    _ => {
                        if !has_event_playback {
                            continue;
                        }
                    }
                }

                // Parse and reconstruct the message with batch tag
                // Build a slirc_proto::Message from the envelope
                let mut tags_vec = vec![Tag::new("batch", Some(batch_id.clone()))];

                // Add stored tags (server-time, msgid, etc.)
                if let Some(envelope_tags) = &msg.envelope.tags {
                    for tag in envelope_tags {
                        tags_vec.push(Tag::new(&tag.key, tag.value.clone()));
                    }
                }

                // Parse command from envelope
                let command = match msg.envelope.command.as_str() {
                    "PRIVMSG" => {
                        Command::PRIVMSG(msg.envelope.target.clone(), msg.envelope.text.clone())
                    }
                    "NOTICE" => {
                        Command::NOTICE(msg.envelope.target.clone(), msg.envelope.text.clone())
                    }
                    "TOPIC" => {
                        // TOPIC command with channel and topic params
                        Command::TOPIC(msg.envelope.target.clone(), Some(msg.envelope.text.clone()))
                    }
                    "TAGMSG" => Command::TAGMSG(msg.envelope.target.clone()),
                    _ => {
                        // Fallback: unknown command type, skip it
                        warn!("Unknown command type in history: {}", msg.envelope.command);
                        continue;
                    }
                };

                // Parse prefix
                let prefix = if !msg.envelope.prefix.is_empty() {
                    // Parse nick!user@host format
                    if let Some(exclaim_pos) = msg.envelope.prefix.find('!') {
                        let nick = msg.envelope.prefix[..exclaim_pos].to_string();
                        if let Some(at_pos) = msg.envelope.prefix[exclaim_pos..].find('@') {
                            let user = msg.envelope.prefix[exclaim_pos + 1..exclaim_pos + at_pos]
                                .to_string();
                            let host = msg.envelope.prefix[exclaim_pos + at_pos + 1..].to_string();
                            Some(Prefix::Nickname(nick, user, host))
                        } else {
                            Some(Prefix::Nickname(nick, String::new(), String::new()))
                        }
                    } else {
                        Some(Prefix::ServerName(msg.envelope.prefix.clone()))
                    }
                } else {
                    None
                };

                let history_msg = Message {
                    tags: Some(tags_vec),
                    prefix,
                    command,
                };

                let _ = ctx.transport.write_message(&history_msg).await;
            }

            // End BATCH
            let batch_end = Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name)),
                command: Command::BATCH(format!("-{}", batch_id), None, None),
            };
            let _ = ctx.transport.write_message(&batch_end).await;

            // NOTE: When a read-marker manager exists, update it after replay.
            return Ok(());
        }
        Ok(_) => {
            // No messages delivered
            return Ok(());
        }
        Err(e) => {
            warn!(target = %target, error = ?e, "Failed to query history for autoreplay");
        }
    }

    Ok(())
}
