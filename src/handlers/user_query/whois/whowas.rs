//! WHOWAS handler for historical user information.

use crate::handlers::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

/// Handler for WHOWAS command.
///
/// `WHOWAS nickname [count [server]]`
///
/// Returns information about a nickname that no longer exists.
/// Queries the WHOWAS history stored in Matrix.
pub struct WhowasHandler;

#[async_trait]
impl PostRegHandler for WhowasHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        // WHOWAS <nick> [count [server]]
        let target = msg.arg(0).unwrap_or("");

        // Parse count parameter
        // Per RFC 1459/2812 and Modern IRC spec:
        // "If a non-positive number is passed as being <count>, then a full search is done"
        let count_limit: Option<usize> = msg
            .arg(1)
            .and_then(|s| s.parse::<i32>().ok())
            .and_then(|n| {
                if n <= 0 {
                    None // Full search - no limit
                } else {
                    Some((n as usize).min(10)) // Cap positive values at 10
                }
            })
            .or(Some(10)); // Default to 10 if not specified

        if target.is_empty() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_NONICKNAMEGIVEN,
                vec![
                    ctx.state.nick.clone(),
                    "No nickname given".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = &ctx.state.nick; // Guaranteed present in RegisteredState

        // Look up WHOWAS history
        let target_lower = irc_to_lower(target);

        if let Some(entries) = ctx.matrix.whowas.get(&target_lower) {
            let entries_to_show: Vec<_> = if let Some(limit) = count_limit {
                entries.iter().take(limit).cloned().collect()
            } else {
                // Full search - return all entries
                entries.iter().cloned().collect()
            };

            if entries_to_show.is_empty() {
                // No entries found
                let reply = server_reply(
                    server_name,
                    Response::ERR_WASNOSUCHNICK,
                    vec![
                        nick.clone(),
                        target.to_string(),
                        "There was no such nickname".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
            } else {
                // Send RPL_WHOWASUSER for each entry
                for entry in entries_to_show {
                    // RPL_WHOWASUSER (314): <nick> <user> <host> * :<realname>
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOWASUSER,
                        vec![
                            nick.clone(),
                            entry.nick,
                            entry.user,
                            entry.host,
                            "*".to_string(),
                            entry.realname,
                        ],
                    );
                    ctx.sender.send(reply).await?;

                    // RPL_WHOISSERVER (312): <nick> <server> :<server info>
                    // Note: Using same numeric for server info in WHOWAS
                    let reply = server_reply(
                        server_name,
                        Response::RPL_WHOISSERVER,
                        vec![
                            nick.clone(),
                            target.to_string(),
                            entry.server.clone(),
                            format!("Logged out at {}", format_timestamp(entry.logout_time)),
                        ],
                    );
                    ctx.sender.send(reply).await?;
                }
            }
        } else {
            // No history for this nick at all
            let reply = server_reply(
                server_name,
                Response::ERR_WASNOSUCHNICK,
                vec![
                    nick.clone(),
                    target.to_string(),
                    "There was no such nickname".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        }

        // RPL_ENDOFWHOWAS (369): <nick> :End of WHOWAS
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFWHOWAS,
            vec![
                nick.clone(),
                target.to_string(),
                "End of WHOWAS".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Format a Unix timestamp as a human-readable string.
fn format_timestamp(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "unknown".to_string())
}
