//! STATS handler for server statistics.

use super::super::{Context, Handler, HandlerError, HandlerResult, err_notregistered, server_reply};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use std::time::{SystemTime, UNIX_EPOCH};

/// Handler for STATS command.
///
/// `STATS [query [target]]`
///
/// Returns statistics about the server.
pub struct StatsHandler;

#[async_trait]
impl Handler for StatsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // STATS [query]
        let query = msg.arg(0).and_then(|s| s.chars().next());

        let query_char = query.unwrap_or('?');

        match query_char {
            'u' => {
                // RPL_STATSUPTIME (242): Server uptime
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let created = ctx.matrix.server_info.created as u64;
                let uptime = now.saturating_sub(created);

                let days = uptime / 86400;
                let hours = (uptime % 86400) / 3600;
                let minutes = (uptime % 3600) / 60;
                let seconds = uptime % 60;

                let reply = server_reply(
                    server_name,
                    Response::RPL_STATSUPTIME,
                    vec![
                        nick.clone(),
                        format!(
                            "Server Up {} days {}:{:02}:{:02}",
                            days, hours, minutes, seconds
                        ),
                    ],
                );
                ctx.sender.send(reply).await?;
            }
            '?' => {
                // Help - list available queries
                let help_lines = [
                    "*** Available STATS queries:",
                    "*** u - Server uptime",
                    "*** ? - This help message",
                ];
                for line in &help_lines {
                    let reply = server_reply(
                        server_name,
                        Response::RPL_STATSDLINE, // Using generic stats reply
                        vec![nick.clone(), (*line).to_string()],
                    );
                    ctx.sender.send(reply).await?;
                }
            }
            _ => {
                // Unknown query - just return end of stats
            }
        }

        // RPL_ENDOFSTATS (219): <query> :End of STATS report
        let reply = server_reply(
            server_name,
            Response::RPL_ENDOFSTATS,
            vec![
                nick.clone(),
                query_char.to_string(),
                "End of STATS report".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
