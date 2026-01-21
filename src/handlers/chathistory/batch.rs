//! Batch sending logic for CHATHISTORY responses.

use crate::handlers::{Context, HandlerError};

use crate::state::RegisteredState;
use slirc_proto::{BatchSubCommand, Command, Message, Tag};
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

        if let Some(history_msg) = super::helpers::history_item_to_message(
            &item,
            &batch_id,
            target,
            has_event_playback,
        ) {
            ctx.sender.send(history_msg).await?;
            // Logging can be reduced or kept
            // println!("DEBUG_BATCH: sent item...");
        }
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
