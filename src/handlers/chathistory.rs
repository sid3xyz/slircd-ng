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

/// 1 millisecond in nanoseconds.
/// IRC timestamps use millisecond precision (ISO8601 with `.3f`), but we store nanoseconds.
/// When excluding a boundary timestamp, we need to add a full millisecond to ensure
/// messages at the same millisecond but different sub-millisecond times are excluded.
const ONE_MILLISECOND_NS: i64 = 1_000_000;

/// Handler for CHATHISTORY command.
pub struct ChatHistoryHandler;

impl ChatHistoryHandler {
    async fn resolve_dm_key(
        &self,
        ctx: &Context<'_, RegisteredState>,
        nick: &str,
        target: &str,
    ) -> String {
        // Resolve self (nick) to account
        // Prefix with 'a:' for account, 'u:' for unregistered nick to avoid collisions

        let sender_key_part = if let Some(acct) = &ctx.state.account {
            format!("a:{}", slirc_proto::irc_to_lower(acct))
        } else {
            // This should not happen for RegisteredState context, but fallback to nick
            format!("u:{}", slirc_proto::irc_to_lower(nick))
        };

        // Resolve target to account
        let target_lower = slirc_proto::irc_to_lower(target);
        let target_account = if let Some(uid_ref) = ctx.matrix.nicks.get(&target_lower) {
            let uid = uid_ref.value();
            if let Some(user) = ctx.matrix.users.get(uid) {
                let u = user.read().await;
                u.account.clone()
            } else {
                None
            }
        } else {
            None
        };

        let target_key_part = if let Some(acct) = target_account {
            format!("a:{}", slirc_proto::irc_to_lower(&acct))
        } else {
            format!("u:{}", target_lower)
        };

        let mut users = [sender_key_part, target_key_part];
        users.sort();
        format!("dm:{}:{}", users[0], users[1])
    }

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
            self.resolve_dm_key(ctx, nick, target).await
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
                        .map(|ts| ts + 1) // Exclusive: exact nanotime, so +1 ns is sufficient
                }
                Ok(MessageReference::Timestamp(ts)) => {
                    // Timestamps have millisecond precision, add 1ms for exclusivity
                    Some(parse_server_time(&ts) + ONE_MILLISECOND_NS)
                }
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
            self.resolve_dm_key(ctx, nick, target).await
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
            self.resolve_dm_key(ctx, nick, target).await
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
                        .map(|ts| ts + 1) // Exclusive: exact nanotime, so +1 ns is sufficient
                }
                Ok(MessageReference::Timestamp(ts)) => {
                    // Timestamps have millisecond precision, add 1ms for exclusivity
                    Some(parse_server_time(&ts) + ONE_MILLISECOND_NS)
                }
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
            self.resolve_dm_key(ctx, nick, target).await
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
            self.resolve_dm_key(ctx, nick, target).await
        } else {
            target.to_string()
        };

        // Parse references, tracking whether they're timestamps (millisecond precision)
        // or msgids (nanosecond precision) for correct exclusivity offset
        let (ts1, is_ts1_timestamp) = if ref1_str == "*" {
            (None, false)
        } else {
            match MessageReference::parse(ref1_str) {
                Ok(MessageReference::MsgId(id)) => {
                    let ts = ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?;
                    (ts, false) // msgid has nanosecond precision
                }
                Ok(MessageReference::Timestamp(ts)) => {
                    (Some(parse_server_time(&ts)), true) // timestamp has millisecond precision
                }
                _ => (None, false),
            }
        };

        let (ts2, is_ts2_timestamp) = if ref2_str == "*" {
            (None, false)
        } else {
            match MessageReference::parse(ref2_str) {
                Ok(MessageReference::MsgId(id)) => {
                    let ts = ctx.matrix.history.lookup_timestamp(&query_target, &id).await
                        .map_err(|e| HandlerError::Internal(e.to_string()))?;
                    (ts, false) // msgid has nanosecond precision
                }
                Ok(MessageReference::Timestamp(ts)) => {
                    (Some(parse_server_time(&ts)), true) // timestamp has millisecond precision
                }
                _ => (None, false),
            }
        };

        // Determine exclusivity offsets based on precision
        let offset1 = if is_ts1_timestamp { ONE_MILLISECOND_NS } else { 1 };
        let offset2 = if is_ts2_timestamp { ONE_MILLISECOND_NS } else { 1 };

        let (start, end, reverse) = match (ts1, ts2) {
            (Some(t1), Some(t2)) => {
                if t1 > t2 {
                    (Some(t2 + offset2), Some(t1), true)
                } else {
                    (Some(t1 + offset1), Some(t2), false)
                }
            }
            (Some(t1), None) => (Some(t1 + offset1), None, false),
            (None, Some(t2)) => (None, Some(t2), false),
            (None, None) => (None, None, false),
        };

        let query = HistoryQuery {
            target: query_target,
            start,
            end,
            limit: limit as usize,
            reverse,
        };

        let mut msgs = ctx.matrix.history.query(query).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        if reverse {
            msgs.reverse();
        }

        Ok(msgs)
    }

    async fn handle_targets(
        &self,
        ctx: &Context<'_, RegisteredState>,
        nick: &str,
        limit: u32,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let start_str = msg.arg(1).unwrap_or("*");
        let end_str = msg.arg(2).unwrap_or("*");

        let start = if start_str == "*" { 0 } else {
            MessageReference::parse(start_str).ok().and_then(|r| match r {
                MessageReference::Timestamp(ts) => Some(parse_server_time(&ts) + 1), // Exclusive start
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

        let targets = ctx.matrix.history.query_targets(start, end, limit as usize, nick.to_string(), channels).await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        let mut msgs = Vec::new();
        let nick_lower = slirc_proto::irc_to_lower(nick);

        for (target_name, timestamp) in targets {
            let display_target = if target_name.contains('\0') {
                let parts: Vec<&str> = target_name.split('\0').collect();
                if parts.len() == 2 {
                    if parts[0] == nick_lower {
                        parts[1].to_string()
                    } else {
                        parts[0].to_string()
                    }
                } else {
                    target_name.clone()
                }
            } else {
                target_name.clone()
            };

            let dt = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_nanos(timestamp as u64));
            let ts_str = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            let envelope = MessageEnvelope {
                command: "TARGET".to_string(),
                prefix: "".to_string(),
                target: display_target.clone(),
                text: ts_str,
                tags: None,
            };

            msgs.push(StoredMessage {
                msgid: "".to_string(),
                nanotime: timestamp,
                target: display_target,
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
///
/// Filters events based on client capabilities:
/// - Without event-playback: only PRIVMSG and NOTICE
/// - With event-playback: also includes TOPIC, TAGMSG, and future event types
async fn send_history_batch(
    ctx: &mut Context<'_, crate::state::RegisteredState>,
    _nick: &str,
    target: &str,
    messages: Vec<StoredMessage>,
    batch_type: &str,
) -> Result<(), crate::handlers::HandlerError> {
    let server_name = &ctx.matrix.server_info.name;
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
            let history_msg = Message {
                tags: Some(vec![Tag::new("batch", Some(batch_id.clone()))]),
                prefix: None,
                command: Command::Raw("CHATHISTORY".to_string(), vec!["TARGETS".to_string(), msg.envelope.target.clone(), msg.envelope.text.clone()]),
            };
            ctx.sender.send(history_msg).await?;
            continue;
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
                warn!(command = %command_type, "Unknown history command type");
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
        prefix: Some(Prefix::ServerName(server_name.clone())),
        command: Command::BATCH(format!("-{}", batch_id), None, None),
    };
    ctx.sender.send(batch_end).await?;

    Ok(())
}
