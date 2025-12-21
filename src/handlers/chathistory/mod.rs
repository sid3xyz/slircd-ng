//! CHATHISTORY command handler (IRCv3 draft/chathistory).
//!
//! Provides message history retrieval for channels.
//!
//! # Reference
//! - IRCv3 chathistory: <https://ircv3.net/specs/extensions/chathistory>

mod batch;
mod helpers;
mod queries;

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{ChatHistorySubCommand, Command, Message, MessageRef, Response};
use tracing::warn;

use batch::send_history_batch;
use helpers::{QueryParams, FAIL_NOT_IN_CHANNEL, FAIL_QUERY_ERROR, FAIL_UNKNOWN_SUBCOMMAND, MAX_HISTORY_LIMIT};
use queries::QueryExecutor;

/// Handler for CHATHISTORY command.
pub struct ChatHistoryHandler;

#[async_trait]
impl PostRegHandler for ChatHistoryHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let nick = ctx.nick().to_string();

        // CHATHISTORY <subcommand> <target> [params...]
        let subcommand_str = match msg.arg(0) {
            Some(s) => s,
            None => {
                let reply = Response::err_needmoreparams(&nick, "CHATHISTORY")
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("CHATHISTORY", "ERR_NEEDMOREPARAMS", reply)
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
                    prefix: Some(ctx.server_prefix()),
                    command: Command::FAIL(
                        "CHATHISTORY".to_string(),
                        "INVALID_PARAMS".to_string(),
                        vec![format!("{}: {}", FAIL_UNKNOWN_SUBCOMMAND, subcommand_str)],
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
                        .with_prefix(ctx.server_prefix());
                    ctx.send_error("CHATHISTORY", "ERR_NEEDMOREPARAMS", reply)
                        .await?;
                    return Ok(());
                }
            }
        };

        let is_dm = !target.starts_with('#') && !target.starts_with('&');

        // Check if user has access to this target (must be in channel for channels)
        if subcommand != ChatHistorySubCommand::TARGETS && !is_dm {
            let target_lower = slirc_proto::irc_to_lower(&target);
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(ctx.uid)
                .map(|u| u.value().clone());
            if let Some(user_arc) = user_arc {
                let in_channel = {
                    let user = user_arc.read().await;
                    user.channels.contains(&target_lower)
                };

                if !in_channel {
                    // User not in channel - send FAIL
                    let fail = Message {
                        tags: None,
                        prefix: Some(ctx.server_prefix()),
                        command: Command::FAIL(
                            "CHATHISTORY".to_string(),
                            "INVALID_TARGET".to_string(),
                            vec![
                                subcommand_str.to_string(),
                                target.clone(),
                                FAIL_NOT_IN_CHANNEL.to_string(),
                            ],
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
        let messages = QueryExecutor::execute(
            ctx,
            subcommand.clone(),
            QueryParams {
                target: &target,
                nick: &nick,
                limit,
                is_dm,
                msg,
            },
        )
        .await;

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
                    prefix: Some(ctx.server_prefix()),
                    command: Command::FAIL(
                        "CHATHISTORY".to_string(),
                        "MESSAGE_ERROR".to_string(),
                        vec![FAIL_QUERY_ERROR.to_string()],
                    ),
                };
                ctx.sender.send(fail).await?;
            }
        }

        Ok(())
    }
}
