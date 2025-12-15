//! CHATHISTORY command handler (IRCv3 draft/chathistory).
//!
//! Provides message history retrieval for channels.
//!
//! # Reference
//! - IRCv3 chathistory: <https://ircv3.net/specs/extensions/chathistory>

use crate::history::{StoredMessage, MessageEnvelope};
use crate::handlers::{Context, HandlerResult, PostRegHandler, HandlerError};
use crate::state::RegisteredState;
use crate::history::HistoryQuery;
use async_trait::async_trait;
use slirc_proto::{
    BatchSubCommand, ChatHistorySubCommand, Command, Message, MessageRef, MessageReference,
    Prefix, Response, Tag, parse_server_time,
};
use tracing::{debug, warn};
use uuid::Uuid;

/// Maximum messages per CHATHISTORY request.
const MAX_HISTORY_LIMIT: u32 = 100;

/// Handler for CHATHISTORY command.
pub struct ChatHistoryHandler;

impl ChatHistoryHandler {
    async fn handle_latest(
        &self,
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            let mut users = vec![nick.to_string(), target.to_string()];
            users.sort();
            format!("dm:{}:{}", slirc_proto::irc_to_lower(&users[0]), slirc_proto::irc_to_lower(&users[1]))
        } else {
            target.to_string()
        };

        let start = if msgref_str == "*" {
            None
        } else {
            let msgref = MessageReference::parse(msgref_str);
            match msgref {
                Ok(MessageReference::MsgId(id)) => {
                    ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?
                }
                Ok(MessageReference::Timestamp(ts)) => Some(parse_server_time(&ts)),
                _ => None,
            }
        };

        let query = HistoryQuery {
            target: query_target,
            start,
            end: None,
            limit: limit as usize,
            reverse: true,
        };

        let mut msgs = ctx.matrix.history.query(query).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        msgs.reverse();
        Ok(msgs)
    }

    async fn handle_before(
        &self,
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            let mut users = vec![nick.to_string(), target.to_string()];
            users.sort();
            format!("dm:{}:{}", slirc_proto::irc_to_lower(&users[0]), slirc_proto::irc_to_lower(&users[1]))
        } else {
            target.to_string()
        };

        let end = if msgref_str == "*" {
            None
        } else {
            let msgref = MessageReference::parse(msgref_str);
            match msgref {
                Ok(MessageReference::MsgId(id)) => {
                    ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?
                }
                Ok(MessageReference::Timestamp(ts)) => Some(parse_server_time(&ts)),
                _ => None,
            }
        };

        let query = HistoryQuery {
            target: query_target,
            start: None,
            end,
            limit: limit as usize,
            reverse: true,
        };

        let mut msgs = ctx.matrix.history.query(query).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        msgs.reverse();
        Ok(msgs)
    }

    async fn handle_after(
        &self,
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            let mut users = vec![nick.to_string(), target.to_string()];
            users.sort();
            format!("dm:{}:{}", slirc_proto::irc_to_lower(&users[0]), slirc_proto::irc_to_lower(&users[1]))
        } else {
            target.to_string()
        };

        let start = if msgref_str == "*" {
            None
        } else {
            let msgref = MessageReference::parse(msgref_str);
            match msgref {
                Ok(MessageReference::MsgId(id)) => {
                    ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?
                }
                Ok(MessageReference::Timestamp(ts)) => Some(parse_server_time(&ts)),
                _ => None,
            }
        };

        let query = HistoryQuery {
            target: query_target,
            start,
            end: None,
            limit: limit as usize,
            reverse: false,
        };

        let msgs = ctx.matrix.history.query(query).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        Ok(msgs)
    }

    async fn handle_around(
        &self,
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            let mut users = vec![nick.to_string(), target.to_string()];
            users.sort();
            format!("dm:{}:{}", slirc_proto::irc_to_lower(&users[0]), slirc_proto::irc_to_lower(&users[1]))
        } else {
            target.to_string()
        };

        let center_ts = if msgref_str == "*" {
            None
        } else {
            match MessageReference::parse(msgref_str) {
                Ok(MessageReference::MsgId(id)) => {
                    ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?
                }
                Ok(MessageReference::Timestamp(ts)) => Some(parse_server_time(&ts)),
                _ => None,
            }
        };

        let center_ts = center_ts.unwrap_or(0);

        let limit_before = limit / 2;
        let limit_after = limit - limit_before;

        let before_query = HistoryQuery {
            target: query_target.clone(),
            start: None,
            end: Some(center_ts),
            limit: limit_before as usize,
            reverse: true,
        };
        let mut before = ctx.matrix.history.query(before_query).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        before.reverse();

        let after_query = HistoryQuery {
            target: query_target,
            start: Some(center_ts),
            end: None,
            limit: limit_after as usize,
            reverse: false,
        };
        let after = ctx.matrix.history.query(after_query).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        before.extend(after);
        Ok(before)
    }

    async fn handle_between(
        &self,
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let ref1_str = msg.arg(2).unwrap_or("*");
        let ref2_str = msg.arg(3).unwrap_or("*");

        let query_target = if is_dm {
            let mut users = vec![nick.to_string(), target.to_string()];
            users.sort();
            format!("dm:{}:{}", slirc_proto::irc_to_lower(&users[0]), slirc_proto::irc_to_lower(&users[1]))
        } else {
            target.to_string()
        };

        // Helper to resolve timestamp
        // We can't use closure easily with async in this context without boxing or complex types.
        // Just duplicate logic or use a helper method.
        // I'll duplicate for now to keep it simple.

        let start_ts = if ref1_str == "*" {
            None
        } else {
            match MessageReference::parse(ref1_str) {
                Ok(MessageReference::MsgId(id)) => {
                    ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?
                }
                Ok(MessageReference::Timestamp(ts)) => Some(parse_server_time(&ts)),
                _ => None,
            }
        };

        let end_ts = if ref2_str == "*" {
            None
        } else {
            match MessageReference::parse(ref2_str) {
                Ok(MessageReference::MsgId(id)) => {
                    ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?
                }
                Ok(MessageReference::Timestamp(ts)) => Some(parse_server_time(&ts)),
                _ => None,
            }
        };

        let (start, end, reverse) = match (start_ts, end_ts) {
            (Some(s), Some(e)) => {
                if s < e {
                    (Some(s), Some(e), false)
                } else {
                    (Some(e), Some(s), true)
                }
            }
            (Some(s), None) => (Some(s), None, false),
            (None, Some(e)) => (None, Some(e), true),
            (None, None) => (None, None, false),
        };

        let query = HistoryQuery {
            target: query_target,
            start,
            end,
            limit: limit as usize,
            reverse,
        };

        let msgs = ctx.matrix.history.query(query).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        Ok(msgs)
    }

    async fn handle_targets(
        &self,
        ctx: &Context<'_, RegisteredState>,
        _nick: &str,
        limit: u32,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let start_str = msg.arg(1).unwrap_or("*");
        let end_str = msg.arg(2).unwrap_or("*");

        let start = if start_str == "*" { 0 } else {
            MessageReference::parse(start_str).ok().and_then(|r| match r {
                MessageReference::Timestamp(ts) => Some(parse_server_time(&ts)),
                _ => None
            }).unwrap_or(0)
        };

        let end = if end_str == "*" { i64::MAX } else {
            MessageReference::parse(end_str).ok().and_then(|r| match r {
                MessageReference::Timestamp(ts) => Some(parse_server_time(&ts)),
                _ => None
            }).unwrap_or(i64::MAX)
        };

        let user_arc = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone());
        let channels = if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
            user.channels.iter().cloned().collect::<Vec<_>>()
        } else {
            vec![]
        };

        let targets = ctx.matrix.history.query_targets(start, end, limit as usize, channels).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        let mut msgs = Vec::new();
        for (target_name, timestamp) in targets {
            let dt = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_nanos(timestamp as u64));
            let ts_str = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            let envelope = MessageEnvelope {
                command: "TARGET".to_string(),
                prefix: "".to_string(),
                target: target_name.clone(),
                text: ts_str,
                tags: None,
            };

            msgs.push(StoredMessage {
                msgid: "".to_string(),
                nanotime: timestamp,
                target: target_name,
                sender: "".to_string(),
                account: None,
                envelope,
            });
        }

        Ok(msgs)
    }

    #[allow(clippy::too_many_arguments)] // Complex query dispatch needs all context
    async fn execute_query(
        &self,
        ctx: &Context<'_, RegisteredState>,
        subcommand: ChatHistorySubCommand,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        match subcommand {
            ChatHistorySubCommand::LATEST => self.handle_latest(ctx, target, nick, limit, is_dm, msg).await,
            ChatHistorySubCommand::BEFORE => self.handle_before(ctx, target, nick, limit, is_dm, msg).await,
            ChatHistorySubCommand::AFTER => self.handle_after(ctx, target, nick, limit, is_dm, msg).await,
            ChatHistorySubCommand::AROUND => self.handle_around(ctx, target, nick, limit, is_dm, msg).await,
            ChatHistorySubCommand::BETWEEN => self.handle_between(ctx, target, nick, limit, is_dm, msg).await,
            ChatHistorySubCommand::TARGETS => self.handle_targets(ctx, nick, limit, msg).await,
            _ => {
                debug!("Unknown CHATHISTORY subcommand");
                Ok(vec![])
            }
        }
    }
}

#[async_trait]
impl PostRegHandler for ChatHistoryHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick().to_string();
        let server_name = &ctx.matrix.server_info.name;

        // CHATHISTORY <subcommand> <target> [params...]
        let subcommand_str = match msg.arg(0) {
            Some(s) => s,
            None => {
                let reply = Response::err_needmoreparams(&nick, "CHATHISTORY")
                    .with_prefix(Prefix::ServerName(server_name.clone()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHATHISTORY", "ERR_NEEDMOREPARAMS");
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

        let target = if subcommand == ChatHistorySubCommand::TARGETS {
            "*".to_string() // Dummy target for TARGETS command
        } else {
            match msg.arg(1) {
                Some(t) => t.to_string(),
                None => {
                    let reply = Response::err_needmoreparams(&nick, "CHATHISTORY")
                        .with_prefix(Prefix::ServerName(server_name.clone()));
                    ctx.sender.send(reply).await?;
                    crate::metrics::record_command_error("CHATHISTORY", "ERR_NEEDMOREPARAMS");
                    return Ok(());
                }
            }
        };

        let is_dm = !target.starts_with('#') && !target.starts_with('&');

        // Check if user has access to this target (must be in channel for channels)
        if subcommand != ChatHistorySubCommand::TARGETS && !is_dm {
            let target_lower = slirc_proto::irc_to_lower(&target);
            let user_arc = ctx.matrix.users.get(ctx.uid).map(|u| u.value().clone());
            if let Some(user_arc) = user_arc {
                let in_channel = {
                    let user = user_arc.read().await;
                    user.channels.contains(&target_lower)
                };

                if !in_channel {
                    // User not in channel - send FAIL
                    let fail = Message {
                        tags: None,
                        prefix: Some(Prefix::ServerName(server_name.clone())),
                        command: Command::FAIL(
                            "CHATHISTORY".to_string(),
                            "INVALID_TARGET".to_string(),
                            vec![subcommand_str.to_string(), target.clone(), "You are not in that channel".to_string()],
                        ),
                    };
                    ctx.sender.send(fail).await?;
                    return Ok(());
                }
            }
        }

        // Parse limit (last argument)
        let limit = match subcommand {
            ChatHistorySubCommand::TARGETS => msg.arg(3).and_then(|s| s.parse().ok()).unwrap_or(50),
            ChatHistorySubCommand::BETWEEN => msg.arg(4).and_then(|s| s.parse().ok()).unwrap_or(50),
            _ => msg.arg(3).and_then(|s| s.parse().ok()).unwrap_or(50),
        };
        let limit = limit.min(MAX_HISTORY_LIMIT);

        // Execute query based on subcommand
        let messages = self.execute_query(ctx, subcommand.clone(), &target, &nick, limit, is_dm, msg).await;

        match messages {
            Ok(msgs) => {
                let batch_type = if subcommand == ChatHistorySubCommand::TARGETS {
                    "draft/chathistory-targets"
                } else {
                    "chathistory"
                };
                send_history_batch(ctx, &nick, &target, msgs, batch_type).await?;
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
    ctx: &mut Context<'_, crate::state::RegisteredState>,
    _nick: &str,
    target: &str,
    messages: Vec<StoredMessage>,
    batch_type: &str,
) -> Result<(), crate::handlers::HandlerError> {
    let server_name = &ctx.matrix.server_info.name;
    let batch_id = format!("chathistory-{}", Uuid::new_v4().simple());

    // Start BATCH
    let batch_params = if batch_type == "draft/chathistory-targets" {
        None
    } else {
        Some(vec![target.to_string()])
    };

    let batch_start = Message {
        tags: None,
        prefix: Some(Prefix::ServerName(server_name.clone())),
        command: Command::BATCH(
            format!("+{}", batch_id),
            Some(BatchSubCommand::CUSTOM(batch_type.to_string())),
            batch_params,
        ),
    };
    ctx.sender.send(batch_start).await?;

    // Send each message with batch tag
    for msg in messages {
        if msg.envelope.command == "TARGET" {
            // Special handling for TARGETS response
            // Format: CHATHISTORY TARGETS <target> <timestamp>
            // We stored target in `target` and timestamp in `text`
            let history_msg = Message {
                tags: Some(vec![Tag::new("batch", Some(batch_id.clone()))]),
                prefix: None,
                command: Command::Raw("CHATHISTORY".to_string(), vec!["TARGETS".to_string(), msg.envelope.target.clone(), msg.envelope.text.clone()]),
            };
            ctx.sender.send(history_msg).await?;
            continue;
        }

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
