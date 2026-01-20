//! Query implementations for CHATHISTORY subcommands.

use crate::handlers::{Context, HandlerError};
use crate::history::{HistoryQuery, MessageEnvelope, StoredMessage, types::HistoryItem};
use crate::state::RegisteredState;
use slirc_proto::{ChatHistorySubCommand, MessageReference, parse_server_time};
use tracing::debug;

use super::helpers::{QueryParams, exclusivity_offset, resolve_dm_key, resolve_msgref};
use super::slicing::slice_around;

/// Implements all CHATHISTORY query operations.
pub struct QueryExecutor;

impl QueryExecutor {
    pub async fn execute(
        ctx: &Context<'_, RegisteredState>,
        subcommand: ChatHistorySubCommand,
        params: QueryParams,
    ) -> Result<Vec<HistoryItem>, HandlerError> {
        let QueryParams {
            target,
            nick,
            limit,
            is_dm,
            msgref,
            msgref2,
        } = params;
        match subcommand {
            ChatHistorySubCommand::LATEST => {
                Self::handle_latest(ctx, &target, &nick, limit, is_dm, &msgref).await
            }
            ChatHistorySubCommand::BEFORE => {
                Self::handle_before(ctx, &target, &nick, limit, is_dm, &msgref).await
            }
            ChatHistorySubCommand::AFTER => {
                Self::handle_after(ctx, &target, &nick, limit, is_dm, &msgref).await
            }
            ChatHistorySubCommand::AROUND => {
                Self::handle_around(ctx, &target, &nick, limit, is_dm, &msgref).await
            }
            ChatHistorySubCommand::BETWEEN => {
                Self::handle_between(
                    ctx,
                    &target,
                    &nick,
                    limit,
                    is_dm,
                    &msgref,
                    msgref2.as_deref().unwrap_or("*"),
                )
                .await
            }
            ChatHistorySubCommand::TARGETS => {
                Self::handle_targets(
                    ctx,
                    &nick,
                    limit,
                    &msgref,
                    msgref2.as_deref().unwrap_or("*"),
                )
                .await
            }
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
        msgref_str: &str,
    ) -> Result<Vec<HistoryItem>, HandlerError> {
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
            start_id: None,
            end_id: None,
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
        msgref_str: &str,
    ) -> Result<Vec<HistoryItem>, HandlerError> {
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
            start_id: None,
            end_id: None,
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
        msgref_str: &str,
    ) -> Result<Vec<HistoryItem>, HandlerError> {
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
            start_id: None,
            end_id: None,
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
        msgref_str: &str,
    ) -> Result<Vec<HistoryItem>, HandlerError> {
        let query_target = if is_dm {
            resolve_dm_key(ctx, nick, target).await
        } else {
            target.to_string()
        };

        let center_ts = resolve_msgref(ctx, &query_target, msgref_str)
            .await?
            .map(|r| r.timestamp)
            .unwrap_or(0);

        let all_messages_query = HistoryQuery {
            target: query_target.clone(),
            start: None,
            end: None,
            start_id: None,
            end_id: None,
            limit: 500,
            reverse: false,
        };

        let messages = ctx
            .matrix
            .service_manager
            .history
            .query(all_messages_query)
            .await
            .map_err(|e| HandlerError::Internal(e.to_string()))?;

        Ok(slice_around(
            messages,
            limit as usize,
            msgref_str,
            center_ts,
        ))
    }

    async fn handle_between(
        ctx: &Context<'_, RegisteredState>,
        target: &str,
        nick: &str,
        limit: u32,
        is_dm: bool,
        ref1_str: &str,
        ref2_str: &str,
    ) -> Result<Vec<HistoryItem>, HandlerError> {
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
            start_id: None,
            end_id: None,
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
        start_str: &str,
        end_str: &str,
    ) -> Result<Vec<HistoryItem>, HandlerError> {
        // Parse start parameter
        // If "*", default to 30 days ago (staleness filter)
        // Otherwise, use explicit timestamp
        let start = if start_str == "*" {
            let now = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
            now - (30 * 24 * 60 * 60 * 1_000_000_000i64) // 30 days ago
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
        let _nick_lower = slirc_proto::irc_to_lower(nick);

        for (target_name, timestamp) in targets {
            let dt = chrono::DateTime::<chrono::Utc>::from(
                std::time::SystemTime::UNIX_EPOCH
                    + std::time::Duration::from_nanos(timestamp as u64),
            );
            let ts_str = dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

            let envelope = MessageEnvelope {
                command: "TARGET".to_string(),
                prefix: "".to_string(),
                target: target_name.clone(),
                text: ts_str,
                tags: None,
            };

            msgs.push(HistoryItem::Message(StoredMessage {
                msgid: "".to_string(),
                nanotime: timestamp,
                target: target_name,
                sender: "".to_string(),
                account: None,
                envelope,
            }));
        }

        Ok(msgs)
    }
}