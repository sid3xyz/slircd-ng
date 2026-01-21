//! Batch sending logic for CHATHISTORY responses.

use crate::handlers::{Context, HandlerError};

use crate::state::RegisteredState;
use slirc_proto::{BatchSubCommand, Command, Message, Prefix, Tag};
use tracing::warn;
use uuid::Uuid;

/// Send history messages wrapped in a BATCH.
///
/// Filters events based on client capabilities:
/// - Without event-playback: only PRIVMSG and NOTICE
/// - With event-playback: also includes TOPIC, TAGMSG, and future event types
pub async fn send_history_batch(
    ctx: &mut Context<'_, RegisteredState>,
    _nick: &str,
    target: &str,
    items: Vec<crate::history::types::HistoryItem>,
    batch_type: &str,
) -> Result<(), HandlerError> {
    let batch_id = format!("chathistory-{}", Uuid::new_v4().simple());

    // Check if client has event-playback capability
    let has_event_playback = ctx.state.capabilities.contains("draft/event-playback");

    // Start BATCH
    let batch_params = if batch_type == "draft/chathistory-targets" {
        None
    } else {
        Some(vec![target.to_string()])
    };

    let batch_start = Message {
        tags: None,
        prefix: Some(ctx.server_prefix()),
        command: Command::BATCH(
            format!("+{}", batch_id),
            Some(BatchSubCommand::CUSTOM(batch_type.to_string())),
            batch_params,
        ),
    };
    ctx.sender.send(batch_start).await?;
    println!("DEBUG_BATCH: sent start batch");

    let expected_target_lower = slirc_proto::irc_to_lower(target);

    // Send each message/event with batch tag
    for item in items {
        if let crate::history::types::HistoryItem::Message(msg) = &item {
            if msg.envelope.command == "TARGET" {
                // Formatting for TARGETS response
                let timestamp = msg.envelope.text.clone();
                let history_msg = Message {
                    tags: Some(vec![Tag::new("batch", Some(batch_id.clone()))]),
                    prefix: Some(ctx.server_prefix()),
                    command: Command::ChatHistoryTargets {
                        target: msg.target.clone(),
                        timestamp,
                    },
                };
                ctx.sender.send(history_msg).await?;
                continue;
            }

            if !msg.target.is_empty() && msg.target != expected_target_lower {
                warn!(
                    expected = %expected_target_lower,
                    db_target = %msg.target,
                    env_target = %msg.envelope.target,
                    msgid = %msg.msgid,
                    "History target mismatch"
                );
            }
        }

        use crate::history::types::{EventKind, HistoryItem};

        // Determine if we should skip this item based on capabilities
        match &item {
            HistoryItem::Message(msg) => {
                let cmd = msg.envelope.command.as_str();
                if (cmd == "TOPIC" || cmd == "TAGMSG") && !has_event_playback {
                    continue;
                }
            }
            HistoryItem::Event(_) => {
                if !has_event_playback {
                    continue;
                }
            }
        }

        // Common tags
        let (nanotime, msgid) = match &item {
            HistoryItem::Message(m) => (m.nanotime, m.msgid.clone()),
            HistoryItem::Event(e) => (e.nanotime, e.id.clone()),
        };

        // Timestamp ISO string
        let time_iso = {
            let secs = nanotime / 1_000_000_000;
            let nanos = (nanotime % 1_000_000_000) as u32;
            if let Some(dt) = chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos) {
                dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            } else {
                "1970-01-01T00:00:00.000Z".to_string()
            }
        };

        let mut tags = vec![
            Tag::new("batch", Some(batch_id.clone())),
            Tag::new("time", Some(time_iso)),
            Tag::new("msgid", Some(msgid.clone())),
        ];

        // Construct command
        let (prefix, command) = match item {
            HistoryItem::Message(msg) => {
                if let Some(account) = &msg.account {
                    tags.push(Tag::new("account", Some(account.clone())));
                }

                // Add preserved client-only tags for TAGMSG
                if let Some(env_tags) = &msg.envelope.tags {
                    for env_tag in env_tags {
                        if env_tag.key.starts_with('+') {
                            tags.push(Tag::new(&env_tag.key, env_tag.value.clone()));
                        }
                    }
                }

                let cmd = match msg.envelope.command.as_str() {
                    "PRIVMSG" => {
                        Command::PRIVMSG(msg.envelope.target.clone(), msg.envelope.text.clone())
                    }
                    "NOTICE" => {
                        Command::NOTICE(msg.envelope.target.clone(), msg.envelope.text.clone())
                    }
                    "TAGMSG" => Command::TAGMSG(msg.envelope.target.clone()),
                    _ => continue,
                };
                (Some(Prefix::new_from_str(&msg.envelope.prefix)), cmd)
            }
            HistoryItem::Event(evt) => {
                let cmd = match evt.kind {
                    EventKind::Join => Command::JOIN(target.to_string(), None, None),
                    EventKind::Part(reason) => Command::PART(target.to_string(), reason),
                    EventKind::Quit(reason) => Command::QUIT(reason),
                    EventKind::Kick {
                        target: kicked,
                        reason,
                    } => Command::KICK(target.to_string(), kicked, reason),
                    EventKind::Mode { diff } => {
                        Command::Raw("MODE".to_string(), vec![target.to_string(), diff])
                    }
                    EventKind::Topic { new_topic, .. } => {
                        Command::TOPIC(target.to_string(), Some(new_topic))
                    }
                    EventKind::Nick { new_nick } => Command::NICK(new_nick),
                };
                (Some(Prefix::new_from_str(&evt.source)), cmd)
            }
        };

        let history_msg = Message {
            tags: Some(tags),
            prefix,
            command,
        };
        ctx.sender.send(history_msg).await?;
        println!("DEBUG_BATCH: sent item {}/{}", nanotime, msgid);
    }

    println!("DEBUG_BATCH: loop finished, sending end");

    // End BATCH
    let batch_end = Message {
        tags: None,
        prefix: Some(ctx.server_prefix()),
        command: Command::BATCH(format!("-{}", batch_id), None, None),
    };
    ctx.sender.send(batch_end).await?;

    Ok(())
}
