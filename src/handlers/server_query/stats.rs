//! STATS handler for server statistics.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use std::time::{SystemTime, UNIX_EPOCH};

/// Handler for STATS command.
///
/// `STATS [query [target]]`
///
/// Returns statistics about the server.
///
/// Supported queries:
/// - `u` - Server uptime
/// - `o` - Online operators
/// - `k` - K-lines (local bans)
/// - `g` - G-lines (global bans)
/// - `z` - Z-lines (IP bans)
/// - `c` - Connection statistics
/// - `m` - Command usage statistics
/// - `?` - Help
pub struct StatsHandler;

#[async_trait]
impl PostRegHandler for StatsHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick();

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

                ctx.send_reply(
                    Response::RPL_STATSUPTIME,
                    vec![
                        nick.to_string(),
                        format!(
                            "Server Up {} days {}:{:02}:{:02}",
                            days, hours, minutes, seconds
                        ),
                    ],
                )
                .await?;
            }
            'o' | 'O' => {
                // RPL_STATSOLINE (243): List online operators
                for user_entry in ctx.matrix.users.iter() {
                    let user_guard = user_entry.value().read().await;
                    if user_guard.modes.oper {
                        // :server 243 nick O * <oper_nick> * :<realname>
                        ctx.send_reply(
                            Response::RPL_STATSOLINE,
                            vec![
                                nick.to_string(),
                                "O".to_string(),
                                "*".to_string(),
                                user_guard.nick.clone(),
                                "*".to_string(),
                                user_guard.realname.clone(),
                            ],
                        )
                        .await?;
                    }
                }
            }
            'k' | 'K' => {
                // RPL_STATSKLINE (216): List K-lines
                if let Ok(klines) = ctx.db.bans().get_active_klines().await {
                    for kline in klines {
                        let duration = kline.expires_at.map(|exp| exp - kline.set_at).unwrap_or(0);
                        let reason = kline.reason.unwrap_or_default();
                        // :server 216 nick K <mask> <set_at> <duration> <setter> :<reason>
                        ctx.send_reply(
                            Response::RPL_STATSKLINE,
                            vec![
                                nick.to_string(),
                                "K".to_string(),
                                kline.mask,
                                kline.set_at.to_string(),
                                duration.to_string(),
                                kline.set_by,
                                reason,
                            ],
                        )
                        .await?;
                    }
                }
            }
            'g' | 'G' => {
                // RPL_STATSDLINE (220) for G-lines (using generic stats reply)
                // Note: Some servers use 223 for G-lines, but slirc-proto uses RPL_STATSDLINE
                if let Ok(glines) = ctx.db.bans().get_active_glines().await {
                    for gline in glines {
                        let duration = gline.expires_at.map(|exp| exp - gline.set_at).unwrap_or(0);
                        let reason = gline.reason.unwrap_or_default();
                        // :server 220 nick G <mask> <set_at> <duration> <setter> :<reason>
                        ctx.send_reply(
                            Response::RPL_STATSDLINE,
                            vec![
                                nick.to_string(),
                                "G".to_string(),
                                gline.mask,
                                gline.set_at.to_string(),
                                duration.to_string(),
                                gline.set_by,
                                reason,
                            ],
                        )
                        .await?;
                    }
                }
            }
            'z' | 'Z' => {
                // Z-lines (IP bans) - using RPL_STATSDLINE
                if let Ok(zlines) = ctx.db.bans().get_active_zlines().await {
                    for zline in zlines {
                        let duration = zline.expires_at.map(|exp| exp - zline.set_at).unwrap_or(0);
                        let reason = zline.reason.unwrap_or_default();
                        // :server 220 nick Z <mask> <set_at> <duration> <setter> :<reason>
                        ctx.send_reply(
                            Response::RPL_STATSDLINE,
                            vec![
                                nick.to_string(),
                                "Z".to_string(),
                                zline.mask,
                                zline.set_at.to_string(),
                                duration.to_string(),
                                zline.set_by,
                                reason,
                            ],
                        )
                        .await?;
                    }
                }
            }
            'c' | 'C' => {
                // Connection statistics
                let current_users = ctx.matrix.users.len();
                let current_channels = ctx.matrix.channels.len();

                // Use RPL_LUSERCLIENT style for connection info
                ctx.send_reply(
                    Response::RPL_LUSERCLIENT,
                    vec![
                        nick.to_string(),
                        format!(
                            "Current local users: {} | Channels: {}",
                            current_users, current_channels
                        ),
                    ],
                )
                .await?;
            }
            'm' | 'M' => {
                // RPL_STATSCOMMANDS (212): Command usage statistics
                let stats = ctx.registry.get_command_stats();

                if stats.is_empty() {
                    ctx.send_reply(
                        Response::RPL_STATSCOMMANDS,
                        vec![nick.to_string(), "No commands have been used yet".to_string()],
                    )
                    .await?;
                } else {
                    for (cmd, count) in stats {
                        // :server 212 nick <command> <count> <byte_count> <remote_count>
                        // We don't track byte_count and remote_count, so use 0
                        ctx.send_reply(
                            Response::RPL_STATSCOMMANDS,
                            vec![
                                nick.to_string(),
                                cmd.to_string(),
                                count.to_string(),
                                "0".to_string(),
                                "0".to_string(),
                            ],
                        )
                        .await?;
                    }
                }
            }
            '?' => {
                // Help - list available queries
                let help_lines = [
                    "*** Available STATS queries:",
                    "*** u - Server uptime",
                    "*** o - Online operators",
                    "*** k - K-lines (local bans)",
                    "*** g - G-lines (global bans)",
                    "*** z - Z-lines (IP bans)",
                    "*** c - Connection statistics",
                    "*** m - Command usage statistics",
                    "*** ? - This help message",
                ];
                for line in &help_lines {
                    ctx.send_reply(
                        Response::RPL_STATSDLINE, // Using generic stats reply
                        vec![nick.to_string(), (*line).to_string()],
                    )
                    .await?;
                }
            }
            _ => {
                // Unknown query - just return end of stats
            }
        }

        // RPL_ENDOFSTATS (219): <query> :End of STATS report
        ctx.send_reply(
            Response::RPL_ENDOFSTATS,
            vec![
                nick.to_string(),
                query_char.to_string(),
                "End of STATS report".to_string(),
            ],
        )
        .await?;

        Ok(())
    }
}
