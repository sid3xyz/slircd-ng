//! CHATHISTORY command handler (IRCv3 draft/chathistory).
//!
//! Provides message history retrieval for channels.
//!
//! # Reference
//! - IRCv3 chathistory: <https://ircv3.net/specs/extensions/chathistory>

use crate::db::{DbError, StoredMessage};
use crate::handlers::{Context, HandlerResult, PostRegHandler, err_needmoreparams};
use crate::handlers::core::traits::TypedContext;
use crate::state::Registered;
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

impl ChatHistoryHandler {
    async fn execute_query(
        &self,
        ctx: &TypedContext<'_, Registered>,
        subcommand: ChatHistorySubCommand,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, DbError> {
        let account = ctx.handshake.account.as_deref();
        match subcommand {
            ChatHistorySubCommand::LATEST => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                if msgref_str == "*" {
                    if is_dm {
                        ctx.db.history().query_dm_latest(nick, account, target, limit).await
                    } else {
                        ctx.db.history().query_latest(target, limit).await
                    }
                } else {
                    let msgref = MessageReference::parse(msgref_str);
                    match msgref {
                        Ok(MessageReference::MsgId(id)) => {
                            let nanos = if is_dm {
                                ctx.db.history().lookup_dm_msgid_nanotime(nick, target, &id).await?
                            } else {
                                ctx.db.history().lookup_msgid_nanotime(target, &id).await?
                            };

                            if let Some(n) = nanos {
                                if is_dm {
                                    ctx.db.history().query_dm_latest_after(nick, account, target, n, limit).await
                                } else {
                                    ctx.db.history().query_latest_after(target, n, limit).await
                                }
                            } else {
                                if is_dm {
                                    ctx.db.history().query_dm_latest(nick, account, target, limit).await
                                } else {
                                    ctx.db.history().query_latest(target, limit).await
                                }
                            }
                        }
                        Ok(MessageReference::Timestamp(ts)) => {
                            let nanos = parse_timestamp_to_nanos(&ts);
                            if is_dm {
                                ctx.db.history().query_dm_latest_after(nick, account, target, nanos, limit).await
                            } else {
                                ctx.db.history().query_latest_after(target, nanos, limit).await
                            }
                        }
                        _ => if is_dm {
                            ctx.db.history().query_dm_latest(nick, account, target, limit).await
                        } else {
                            ctx.db.history().query_latest(target, limit).await
                        },
                    }
                }
            }
            ChatHistorySubCommand::BEFORE => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                let msgref = MessageReference::parse(msgref_str);
                let nanos = match msgref {
                    Ok(MessageReference::MsgId(id)) => {
                        let n = if is_dm {
                            ctx.db.history().lookup_dm_msgid_nanotime(nick, target, &id).await?
                        } else {
                            ctx.db.history().lookup_msgid_nanotime(target, &id).await?
                        };
                        n.unwrap_or(i64::MAX)
                    }
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => i64::MAX,
                };
                if is_dm {
                    ctx.db.history().query_dm_before(nick, account, target, nanos, limit).await
                } else {
                    ctx.db.history().query_before(target, nanos, limit).await
                }
            }
            ChatHistorySubCommand::AFTER => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                let msgref = MessageReference::parse(msgref_str);
                let nanos = match msgref {
                    Ok(MessageReference::MsgId(id)) => {
                        let n = if is_dm {
                            ctx.db.history().lookup_dm_msgid_nanotime(nick, target, &id).await?
                        } else {
                            ctx.db.history().lookup_msgid_nanotime(target, &id).await?
                        };
                        n.unwrap_or(0)
                    }
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => 0,
                };
                if is_dm {
                    ctx.db.history().query_dm_after(nick, account, target, nanos, limit).await
                } else {
                    ctx.db.history().query_after(target, nanos, limit).await
                }
            }
            ChatHistorySubCommand::AROUND => {
                let msgref_str = msg.arg(2).unwrap_or("*");
                let msgref = MessageReference::parse(msgref_str);

                let (nanos, center_msg) = match msgref {
                    Ok(MessageReference::MsgId(id)) => {
                        let n = if is_dm {
                            ctx.db.history().lookup_dm_msgid_nanotime(nick, target, &id).await?
                        } else {
                            ctx.db.history().lookup_msgid_nanotime(target, &id).await?
                        };

                        let msg = if let Some(_) = n {
                            ctx.db.history().get_message_by_id(&id).await?
                        } else {
                            None
                        };

                        debug!("AROUND lookup: id={} n={:?} msg={:?}", id, n, msg.is_some());
                        (n.unwrap_or(0), msg)
                    }
                    Ok(MessageReference::Timestamp(ts)) => {
                        let n = parse_timestamp_to_nanos(&ts);
                        let center = if is_dm {
                            ctx.db.history().query_dm_between(nick, account, target, n - 1, n + 1, 1).await?
                        } else {
                            ctx.db.history().query_between(target, n - 1, n + 1, 1).await?
                        };
                        (n, center.into_iter().next())
                    },
                    _ => (0, None),
                };

                let (before_limit, after_limit) = if center_msg.is_some() {
                    let rem = limit.saturating_sub(1);
                    let b = rem / 2;
                    (b, rem - b)
                } else {
                    let b = limit / 2;
                    (b, limit - b)
                };

                debug!("AROUND limits: limit={} before={} after={}", limit, before_limit, after_limit);

                let mut before = if is_dm {
                    ctx.db.history().query_dm_before(nick, account, target, nanos, before_limit as u32).await?
                } else {
                    ctx.db.history().query_before(target, nanos, before_limit as u32).await?
                };

                let after = if is_dm {
                    ctx.db.history().query_dm_after(nick, account, target, nanos, after_limit as u32).await?
                } else {
                    ctx.db.history().query_after(target, nanos, after_limit as u32).await?
                };

                before.reverse();
                if let Some(m) = center_msg {
                    before.push(m);
                }
                before.extend(after);
                debug!("AROUND results: {}", before.len());
                Ok(before)
            }
            ChatHistorySubCommand::BETWEEN => {
                let ref1_str = msg.arg(2).unwrap_or("*");
                let ref2_str = msg.arg(3).unwrap_or("*");
                let ref1 = MessageReference::parse(ref1_str);
                let ref2 = MessageReference::parse(ref2_str);

                let start_nanos = match ref1 {
                    Ok(MessageReference::MsgId(id)) => {
                        let n = if is_dm {
                            ctx.db.history().lookup_dm_msgid_nanotime(nick, target, &id).await?
                        } else {
                            ctx.db.history().lookup_msgid_nanotime(target, &id).await?
                        };
                        n.unwrap_or(0)
                    }
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => 0,
                };
                let end_nanos = match ref2 {
                    Ok(MessageReference::MsgId(id)) => {
                        let n = if is_dm {
                            ctx.db.history().lookup_dm_msgid_nanotime(nick, target, &id).await?
                        } else {
                            ctx.db.history().lookup_msgid_nanotime(target, &id).await?
                        };
                        n.unwrap_or(i64::MAX)
                    }
                    Ok(MessageReference::Timestamp(ts)) => parse_timestamp_to_nanos(&ts),
                    _ => i64::MAX,
                };

                if start_nanos < end_nanos {
                    if is_dm {
                        ctx.db.history().query_dm_between(nick, account, target, start_nanos, end_nanos, limit).await
                    } else {
                        ctx.db.history().query_between(target, start_nanos, end_nanos, limit).await
                    }
                } else {
                    if is_dm {
                        ctx.db.history().query_dm_between_desc(nick, account, target, end_nanos, start_nanos, limit).await
                    } else {
                        ctx.db.history().query_between_desc(target, end_nanos, start_nanos, limit).await
                    }
                }
            }
            ChatHistorySubCommand::TARGETS => {
                let start_str = msg.arg(1).unwrap_or("*");
                let end_str = msg.arg(2).unwrap_or("*");

                let start = if start_str == "*" { 0 } else {
                    MessageReference::parse(start_str).ok().and_then(|r| match r {
                        MessageReference::Timestamp(ts) => Some(parse_timestamp_to_nanos(&ts)),
                        _ => None
                    }).unwrap_or(0)
                };

                let end = if end_str == "*" { i64::MAX } else {
                    MessageReference::parse(end_str).ok().and_then(|r| match r {
                        MessageReference::Timestamp(ts) => Some(parse_timestamp_to_nanos(&ts)),
                        _ => None
                    }).unwrap_or(i64::MAX)
                };

                let channels = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                    let user = user_ref.read().await;
                    user.channels.iter().cloned().collect::<Vec<_>>()
                } else {
                    vec![]
                };

                let targets = ctx.db.history().query_targets(nick, &channels, start, end, limit as usize).await?;

                let mut msgs = Vec::new();
                for (target_name, timestamp) in targets {
                    let dt = chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_nanos(timestamp as u64));
                    let ts_str = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

                    // Create a dummy StoredMessage for TARGETS
                    // We use "TARGET" as command, target_name as target, and timestamp as text
                    // This is a hack to pass data to send_history_batch
                    let envelope = crate::db::MessageEnvelope {
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
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick().to_string();
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

        let target = if subcommand == ChatHistorySubCommand::TARGETS {
            "*".to_string() // Dummy target for TARGETS command
        } else {
            match msg.arg(1) {
                Some(t) => t.to_string(),
                None => {
                    ctx.sender
                        .send(err_needmoreparams(server_name, &nick, "CHATHISTORY"))
                        .await?;
                    return Ok(());
                }
            }
        };

        let is_dm = !target.starts_with('#') && !target.starts_with('&');

        // Check if user has access to this target (must be in channel for channels)
        if subcommand != ChatHistorySubCommand::TARGETS && !is_dm {
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
    ctx: &mut Context<'_>,
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

/// Parse ISO8601 timestamp to nanoseconds since epoch.
fn parse_timestamp_to_nanos(ts: &str) -> i64 {
    use chrono::DateTime;

    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        dt.timestamp_nanos_opt().unwrap_or(0)
    } else {
        0
    }
}
