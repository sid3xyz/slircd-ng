//! CHATHISTORY command handler (IRCv3 draft/chathistory).
//!
//! Provides message history retrieval for channels.
//!
//! # Reference
//! - IRCv3 chathistory: <https://ircv3.net/specs/extensions/chathistory>

use crate::db::StoredMessage;
use crate::handlers::{Context, Handler, HandlerResult, err_needmoreparams, get_nick_or_star};
use async_trait::async_trait;
use slirc_proto::{
    BatchSubCommand, ChatHistorySubCommand, Command, Message, MessageRef, MessageReference, Prefix,
    Tag,
};
use tracing::{debug, warn};
use uuid::Uuid;

/// Maximum messages per CHATHISTORY request.
const MAX_HISTORY_LIMIT: u32 = 100;

/// Handler for CHATHISTORY command.
pub struct ChatHistoryHandler;

#[async_trait]
impl Handler for ChatHistoryHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = get_nick_or_star(ctx).await;
        let server_name = &ctx.matrix.server_info.name;

        // CHATHISTORY <subcommand> <target> [params...]
        let subcommand_str = match msg.arg(0) {
            Some(s) => s,
            None => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "CHATHISTORY"))
                    .await?;
                return Ok(());
            }
        };

        let subcommand: ChatHistorySubCommand = match subcommand_str.parse() {
            Ok(cmd) => cmd,
            Err(_) => {
                // Send FAIL response for invalid subcommand
                let fail = Message {
                    tags: None,
                    prefix: Some(Prefix::ServerName(server_name.clone())),
                    command: Command::FAIL(
                        "CHATHISTORY".to_string(),
                        "INVALID_PARAMS".to_string(),
                        vec![format!("Unknown subcommand: {}", subcommand_str)],
                    ),
                };
                ctx.sender.send(fail).await?;
                return Ok(());
            }
        };

        let target = match msg.arg(1) {
            Some(t) => t.to_string(),
            None => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "CHATHISTORY"))
                    .await?;
                return Ok(());
            }
        };

        // Check if user has access to this target (must be in channel for channels)
        if target.starts_with('#') || target.starts_with('&') {
            let target_lower = slirc_proto::irc_to_lower(&target);
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                if !user.channels.contains(&target_lower) {
                    // User not in channel - send FAIL
                    let fail = Message {
                        tags: None,
                        prefix: Some(Prefix::ServerName(server_name.clone())),
                        command: Command::FAIL(
                            "CHATHISTORY".to_string(),
                            "INVALID_TARGET".to_string(),
                            vec![target.clone(), "You are not in that channel".to_string()],
                        ),
                    };
                    ctx.sender.send(fail).await?;
                    return Ok(());
                }
            }
        }

        // Parse limit (last argument)
        let limit = match subcommand {
            ChatHistorySubCommand::TARGETS => msg.arg(4).and_then(|s| s.parse().ok()).unwrap_or(50),
            ChatHistorySubCommand::BETWEEN => msg.arg(4).and_then(|s| s.parse().ok()).unwrap_or(50),
            _ => msg.arg(3).and_then(|s| s.parse().ok()).unwrap_or(50),
        };
        let limit = limit.min(MAX_HISTORY_LIMIT);

        // Execute query based on subcommand
        let messages = match subcommand {
            ChatHistorySubCommand::LATEST => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                if msgref_str == "*" {
                    ctx.db.history().query_latest(&target, limit).await
                } else {
                    let msgref = MessageReference::parse(msgref_str);
                    match msgref {
                        Ok(MessageReference::MsgId(id)) => {
                            if let Ok(Some(nanos)) =
                                ctx.db.history().lookup_msgid_nanotime(&target, &id).await
                            {
                                ctx.db.history().query_before(&target, nanos, limit).await
                            } else {
                                ctx.db.history().query_latest(&target, limit).await
                            }
                        }
                        Ok(MessageReference::Timestamp(ts)) => {
                            let nanos = parse_timestamp_to_nanos(&ts);
                            ctx.db.history().query_before(&target, nanos, limit).await
                        }
                        _ => ctx.db.history().query_latest(&target, limit).await,
                    }
                }
            }
            ChatHistorySubCommand::BEFORE => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                let msgref = MessageReference::parse(msgref_str);
                let nanos = match msgref {
                    Ok(MessageReference::MsgId(id)) => ctx
                        .db
                        .history()
                        .lookup_msgid_nanotime(&target, &id)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(i64::MAX),
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => i64::MAX,
                };
                ctx.db.history().query_before(&target, nanos, limit).await
            }
            ChatHistorySubCommand::AFTER => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                let msgref = MessageReference::parse(msgref_str);
                let nanos = match msgref {
                    Ok(MessageReference::MsgId(id)) => ctx
                        .db
                        .history()
                        .lookup_msgid_nanotime(&target, &id)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(0),
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => 0,
                };
                ctx.db.history().query_after(&target, nanos, limit).await
            }
            ChatHistorySubCommand::AROUND => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                let msgref = MessageReference::parse(msgref_str);
                let nanos = match msgref {
                    Ok(MessageReference::MsgId(id)) => ctx
                        .db
                        .history()
                        .lookup_msgid_nanotime(&target, &id)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(0),
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => 0,
                };
                ctx.db.history().query_around(&target, nanos, limit).await
            }
            ChatHistorySubCommand::BETWEEN => {
                let ref1_str = msg.arg(2).unwrap_or("*");
                let ref2_str = msg.arg(3).unwrap_or("*");
                let ref1 = MessageReference::parse(ref1_str);
                let ref2 = MessageReference::parse(ref2_str);

                let start_nanos = match ref1 {
                    Ok(MessageReference::MsgId(id)) => ctx
                        .db
                        .history()
                        .lookup_msgid_nanotime(&target, &id)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(0),
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => 0,
                };
                let end_nanos = match ref2 {
                    Ok(MessageReference::MsgId(id)) => ctx
                        .db
                        .history()
                        .lookup_msgid_nanotime(&target, &id)
                        .await
                        .ok()
                        .flatten()
                        .unwrap_or(i64::MAX),
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => i64::MAX,
                };

                ctx.db
                    .history()
                    .query_between(&target, start_nanos, end_nanos, limit)
                    .await
            }
            ChatHistorySubCommand::TARGETS => {
                // TARGETS is not yet implemented - would require scanning distinct targets
                debug!("CHATHISTORY TARGETS not yet implemented");
                Ok(vec![])
            }
            _ => {
                // Handle any future subcommands added to the non-exhaustive enum
                debug!("Unknown CHATHISTORY subcommand");
                Ok(vec![])
            }
        };

        match messages {
            Ok(msgs) => {
                send_history_batch(ctx, &nick, &target, msgs).await?;
            }
            Err(e) => {
                warn!(error = %e, "CHATHISTORY query failed");
                let fail = Message {
                    tags: None,
                    prefix: Some(Prefix::ServerName(server_name.clone())),
                    command: Command::FAIL(
                        "CHATHISTORY".to_string(),
                        "MESSAGE_ERROR".to_string(),
                        vec!["Failed to retrieve history".to_string()],
                    ),
                };
                ctx.sender.send(fail).await?;
            }
        }

        Ok(())
    }
}

/// Send history messages wrapped in a BATCH.
async fn send_history_batch(
    ctx: &mut Context<'_>,
    _nick: &str,
    target: &str,
    messages: Vec<StoredMessage>,
) -> Result<(), crate::handlers::HandlerError> {
    let server_name = &ctx.matrix.server_info.name;
    let batch_id = format!("chathistory-{}", Uuid::new_v4().simple());

    // Start BATCH
    let batch_start = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.clone())),
        command: Command::BATCH(
            format!("+{}", batch_id),
            Some(BatchSubCommand::CUSTOM("chathistory".to_string())),
            Some(vec![target.to_string()]),
        ),
    };
    ctx.sender.send(batch_start).await?;

    // Send each message with batch tag
    for msg in messages {
        let mut tags = vec![
            Tag::new("batch", Some(batch_id.clone())),
            Tag::new("time", Some(msg.timestamp_iso())),
            Tag::new("msgid", Some(msg.msgid.clone())),
        ];

        if let Some(account) = &msg.account {
            tags.push(Tag::new("account", Some(account.clone())));
        }

        let history_msg = Message {
            tags: Some(tags),
            prefix: Some(Prefix::new_from_str(&msg.envelope.prefix)),
            command: Command::PRIVMSG(msg.envelope.target.clone(), msg.envelope.text.clone()),
        };
        ctx.sender.send(history_msg).await?;
    }

    // End BATCH
    let batch_end = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.clone())),
        command: Command::BATCH(format!("-{}", batch_id), None, None),
    };
    ctx.sender.send(batch_end).await?;

    Ok(())
}

/// Parse ISO8601 timestamp to nanoseconds since epoch.
fn parse_timestamp_to_nanos(ts: &str) -> i64 {
    use chrono::DateTime;

    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        dt.timestamp_nanos_opt().unwrap_or(0)
    } else {
        0
    }
}
