//! Query implementations for CHATHISTORY subcommands.

use crate::handlers::{Context, HandlerError};
use crate::history::{HistoryQuery, MessageEnvelope, StoredMessage};
use crate::state::RegisteredState;
use slirc_proto::{ChatHistorySubCommand, MessageRef, MessageReference, parse_server_time};
use tracing::debug;

use super::helpers::{QueryParams, exclusivity_offset, resolve_dm_key, resolve_msgref};

/// Implements all CHATHISTORY query operations.
pub struct QueryExecutor;

impl QueryExecutor {
    pub async fn execute<'a>(
        ctx: &Context<'_, RegisteredState>,
        subcommand: ChatHistorySubCommand,
        params: QueryParams<'a>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let QueryParams {
            target,
            nick,
            limit,
            is_dm,
            msg,
        } = params;
        match subcommand {
            ChatHistorySubCommand::LATEST => {
                Self::handle_latest(ctx, target, nick, limit, is_dm, msg).await
            }
            ChatHistorySubCommand::BEFORE => {
                Self::handle_before(ctx, target, nick, limit, is_dm, msg).await
            }
            ChatHistorySubCommand::AFTER => {
                Self::handle_after(ctx, target, nick, limit, is_dm, msg).await
            }
            ChatHistorySubCommand::AROUND => {
                Self::handle_around(ctx, target, nick, limit, is_dm, msg).await
            }
            ChatHistorySubCommand::BETWEEN => {
                Self::handle_between(ctx, target, nick, limit, is_dm, msg).await
            }
            ChatHistorySubCommand::TARGETS => Self::handle_targets(ctx, nick, limit, msg).await,
            _ => {
                debug!("Unknown CHATHISTORY subcommand");
                Ok(vec![])
            }
        }
    }

    async fn handle_latest(
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            resolve_dm_key(ctx, nick, target).await
        } else {
            target.to_string()
        };

        let start = resolve_msgref(ctx, &query_target, msgref_str)
            .await?
            .map(|resolved| resolved.timestamp + exclusivity_offset(&resolved));

        let query = HistoryQuery {
            target: query_target,
            start,
            end: None,
            limit: limit as usize,
            reverse: true,
        };

        let mut msgs = ctx
            .matrix
            .service_manager
            .history
            .query(query)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        msgs.reverse();
        Ok(msgs)
    }

    async fn handle_before(
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            resolve_dm_key(ctx, nick, target).await
        } else {
            target.to_string()
        };

        // For BEFORE, we don't add exclusivity offset - we want messages up to but not including
        let end = resolve_msgref(ctx, &query_target, msgref_str)
            .await?
            .map(|r| r.timestamp);

        let query = HistoryQuery {
            target: query_target,
            start: None,
            end,
            limit: limit as usize,
            reverse: true,
        };

        let mut msgs = ctx
            .matrix
            .service_manager
            .history
            .query(query)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        msgs.reverse();
        Ok(msgs)
    }

    async fn handle_after(
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            resolve_dm_key(ctx, nick, target).await
        } else {
            target.to_string()
        };

        // For AFTER, add exclusivity offset to skip the referenced message
        let start = resolve_msgref(ctx, &query_target, msgref_str)
            .await?
            .map(|resolved| resolved.timestamp + exclusivity_offset(&resolved));

        let query = HistoryQuery {
            target: query_target,
            start,
            end: None,
            limit: limit as usize,
            reverse: false,
        };

        let msgs = ctx
            .matrix
            .service_manager
            .history
            .query(query)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        Ok(msgs)
    }

    async fn handle_around(
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let msgref_str = msg.arg(2).unwrap_or("*");

        let query_target = if is_dm {
            resolve_dm_key(ctx, nick, target).await
        } else {
            target.to_string()
        };

        let center_ts = resolve_msgref(ctx, &query_target, msgref_str)
            .await?
            .map(|r| r.timestamp)
            .unwrap_or(0);

        let limit_before = limit / 2;
        let limit_after = limit - limit_before;

        let before_query = HistoryQuery {
            target: query_target.clone(),
            start: None,
            end: Some(center_ts),
            limit: limit_before as usize,
            reverse: true,
        };
        let mut before = ctx
            .matrix
            .service_manager
            .history
            .query(before_query)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;
        before.reverse();

        let after_query = HistoryQuery {
            target: query_target,
            start: Some(center_ts),
            end: None,
            limit: limit_after as usize,
            reverse: false,
        };
        let after = ctx
            .matrix
            .service_manager
            .history
            .query(after_query)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        before.extend(after);
        Ok(before)
    }

    async fn handle_between(
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
            resolve_dm_key(ctx, nick, target).await
        } else {
            target.to_string()
        };

        // Parse both references using the helper
        let resolved1 = resolve_msgref(ctx, &query_target, ref1_str).await?;
        let resolved2 = resolve_msgref(ctx, &query_target, ref2_str).await?;

        // Calculate exclusive boundaries with appropriate precision-based offsets
        let (start, end, reverse) = match (resolved1, resolved2) {
            (Some(r1), Some(r2)) => {
                if r1.timestamp > r2.timestamp {
                    // ref1 is later - query backwards from ref1 to ref2
                    (
                        Some(r2.timestamp + exclusivity_offset(&r2)),
                        Some(r1.timestamp),
                        true,
                    )
                } else {
                    // ref1 is earlier - query forwards from ref1 to ref2
                    (
                        Some(r1.timestamp + exclusivity_offset(&r1)),
                        Some(r2.timestamp),
                        false,
                    )
                }
            }
            (Some(r1), None) => (Some(r1.timestamp + exclusivity_offset(&r1)), None, false),
            (None, Some(r2)) => (None, Some(r2.timestamp), false),
            (None, None) => (None, None, false),
        };

        let query = HistoryQuery {
            target: query_target,
            start,
            end,
            limit: limit as usize,
            reverse,
        };

        let mut msgs = ctx
            .matrix
            .service_manager
            .history
            .query(query)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        if reverse {
            msgs.reverse();
        }

        Ok(msgs)
    }

    async fn handle_targets(
        ctx: &Context<'_, RegisteredState>,
        nick: &str,
        limit: u32,
        msg: &MessageRef<'_>,
    ) -> Result<Vec<StoredMessage>, HandlerError> {
        let start_str = msg.arg(1).unwrap_or("*");
        let end_str = msg.arg(2).unwrap_or("*");

        let start = if start_str == "*" {
            0
        } else {
            MessageReference::parse(start_str)
                .ok()
                .and_then(|r| match r {
                    MessageReference::Timestamp(ts) => Some(parse_server_time(&ts) + 1), // Exclusive start
                    _ => None,
                })
                .unwrap_or(0)
        };

        let end = if end_str == "*" {
            i64::MAX
        } else {
            MessageReference::parse(end_str)
                .ok()
                .and_then(|r| match r {
                    MessageReference::Timestamp(ts) => Some(parse_server_time(&ts)),
                    _ => None,
                })
                .unwrap_or(i64::MAX)
        };

        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(ctx.uid)
            .map(|u| u.value().clone());
        let channels = if let Some(user_arc) = user_arc {
            let user = user_arc.read().await;
            user.channels.iter().cloned().collect::<Vec<_>>()
        } else {
            vec![]
        };

        let targets = ctx
            .matrix
            .service_manager
            .history
            .query_targets(start, end, limit as usize, nick.to_string(), channels)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        let mut msgs = Vec::with_capacity(limit as usize);
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

            let dt = chrono::DateTime::<chrono::Utc>::from(
                std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_nanos(timestamp as u64),
            );
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
}
