//! Batch sending logic for CHATHISTORY responses.

use crate::handlers::{Context, HandlerError};
use crate::history::StoredMessage;
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
    messages: Vec<StoredMessage>,
    batch_type: &str,
) -> Result<(), HandlerError> {
    let batch_id = format!("chathistory-{}", Uuid::new_v4().simple());

    // Check if client has event-playback capability (Innovation 5)
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

    let expected_target_lower = slirc_proto::irc_to_lower(target);

    // Send each message with batch tag
    for msg in messages {
        if msg.envelope.command == "TARGET" {
            // Special handling for TARGETS response
            // Format: CHATHISTORY TARGETS <target> <count>
            // Note: Command::CHATHISTORY is designed for requests, not TARGETS responses
            // which have a different parameter structure. Using Command::Raw is correct here.
            let history_msg = Message {
                tags: Some(vec![Tag::new("batch", Some(batch_id.clone()))]),
                prefix: None,
                command: Command::Raw(
                    "CHATHISTORY".to_string(),
                    vec![
                        "TARGETS".to_string(),
                        msg.target.clone(),
                        msg.envelope.text.clone(),
                    ],
                ),
            };
            ctx.sender.send(history_msg).await?;
            continue;
        }

        // These fields are selected/stored separately for lookup; validate they remain consistent.
        if !msg.target.is_empty() && msg.target != expected_target_lower {
            warn!(
                expected = %expected_target_lower,
                db_target = %msg.target,
                env_target = %msg.envelope.target,
                msgid = %msg.msgid,
                "History target mismatch"
            );
        }
        if !msg.sender.is_empty() && !msg.envelope.prefix.starts_with(&msg.sender) {
            warn!(
                sender = %msg.sender,
                prefix = %msg.envelope.prefix,
                msgid = %msg.msgid,
                "History sender mismatch"
            );
        }

        // Filter events based on event-playback capability
        let command_type = msg.envelope.command.as_str();
        match command_type {
            "PRIVMSG" | "NOTICE" => {
                // Always include messages
            }
            "TOPIC" | "TAGMSG" => {
                // Only include if client has event-playback
                if !has_event_playback {
                    continue;
                }
            }
            _ => {
                // Future event types (JOIN, PART, MODE, etc.) - require event-playback
                if !has_event_playback {
                    continue;
                }
            }
        }

        let mut tags = vec![
            Tag::new("batch", Some(batch_id.clone())),
            Tag::new("time", Some(msg.timestamp_iso())),
            Tag::new("msgid", Some(msg.msgid.clone())),
        ];

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

        // Build the appropriate IRC command based on event type
        let command = match command_type {
            "PRIVMSG" => Command::PRIVMSG(msg.envelope.target.clone(), msg.envelope.text.clone()),
            "NOTICE" => Command::NOTICE(msg.envelope.target.clone(), msg.envelope.text.clone()),
            "TOPIC" => Command::TOPIC(msg.envelope.target.clone(), Some(msg.envelope.text.clone())),
            "TAGMSG" => Command::TAGMSG(msg.envelope.target.clone()),
            _ => {
                // Unknown command type - skip
                warn!(
                    command = %command_type,
                    sender = %msg.sender,
                    target = %msg.target,
                    "Unknown history command type"
                );
                continue;
            }
        };

        let history_msg = Message {
            tags: Some(tags),
            prefix: Some(Prefix::new_from_str(&msg.envelope.prefix)),
            command,
        };
        ctx.sender.send(history_msg).await?;
    }

    // End BATCH
    let batch_end = Message {
        tags: None,
        prefix: Some(ctx.server_prefix()),
        command: Command::BATCH(format!("-{}", batch_id), None, None),
    };
    ctx.sender.send(batch_end).await?;

    Ok(())
}
