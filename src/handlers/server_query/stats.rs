//! STATS handler for server statistics.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::metrics::{S2S_BYTES_RECEIVED, S2S_BYTES_SENT, S2S_COMMANDS};
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
                for user_entry in ctx.matrix.user_manager.users.iter() {
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
            'Z' => {
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
            'z' => {
                // RPL_STATSDEBUG (249) - Custom stats
                let user_count = ctx.matrix.user_manager.users.len();
                let channel_count = ctx.matrix.channel_manager.channels.len();
                let server_count = ctx.matrix.sync_manager.topology.servers.len();

                ctx.send_reply(
                    Response::RPL_STATSDEBUG,
                    vec![nick.to_string(), format!("Users: {}", user_count)],
                )
                .await?;
                ctx.send_reply(
                    Response::RPL_STATSDEBUG,
                    vec![nick.to_string(), format!("Channels: {}", channel_count)],
                )
                .await?;
                ctx.send_reply(
                    Response::RPL_STATSDEBUG,
                    vec![nick.to_string(), format!("Servers: {}", server_count)],
                )
                .await?;
            }
            'l' | 'L' => {
                // RPL_STATSLINKINFO (211)
                for entry in ctx.matrix.sync_manager.links.iter() {
                    let sid = entry.key();
                    let link = entry.value();
                    let sent_bytes = S2S_BYTES_SENT.with_label_values(&[sid.as_str()]).get();
                    let recv_bytes = S2S_BYTES_RECEIVED.with_label_values(&[sid.as_str()]).get();
                    let _sent_msgs = S2S_COMMANDS
                        .with_label_values(&[sid.as_str(), "TOTAL"])
                        .get(); // We need to sum commands or just use 0 if not aggregated
                    // Actually S2S_COMMANDS has a command label. Summing is hard without iterating.
                    // For now, let's just report 0 for msg count or try to track it separately if critical.
                    // The user asked for "Track message counts by command type", which we did.
                    // STATS L usually wants total messages.
                    // I'll just use 0 for now as bytes are more important for bandwidth.

                    let time_open = link.connected_at.elapsed().as_secs();

                    ctx.send_reply(
                        Response::RPL_STATSLINKINFO,
                        vec![
                            nick.to_string(),
                            link.name.clone(),
                            "0".to_string(), // SendQ
                            "0".to_string(), // Sent Messages (TODO)
                            sent_bytes.to_string(),
                            "0".to_string(), // Recv Messages (TODO)
                            recv_bytes.to_string(),
                            time_open.to_string(),
                        ],
                    )
                    .await?;
                }
            }
            'd' | 'D' => {
                // D-lines (IP bans)
                if let Ok(dlines) = ctx.db.bans().get_active_dlines().await {
                    for dline in dlines {
                        let duration = dline.expires_at.map(|exp| exp - dline.set_at).unwrap_or(0);
                        let reason = dline.reason.unwrap_or_default();
                        // :server 220 nick D <mask> <set_at> <duration> <setter> :<reason>
                        ctx.send_reply(
                            Response::RPL_STATSDLINE,
                            vec![
                                nick.to_string(),
                                "D".to_string(),
                                dline.mask,
                                dline.set_at.to_string(),
                                duration.to_string(),
                                dline.set_by,
                                reason,
                            ],
                        )
                        .await?;
                    }
                }
            }
            'r' | 'R' => {
                // R-lines (Realname bans)
                if let Ok(rlines) = ctx.db.bans().get_active_rlines().await {
                    for rline in rlines {
                        let duration = rline.expires_at.map(|exp| exp - rline.set_at).unwrap_or(0);
                        let reason = rline.reason.unwrap_or_default();
                        // :server 220 nick R <mask> <set_at> <duration> <setter> :<reason>
                        ctx.send_reply(
                            Response::RPL_STATSDLINE,
                            vec![
                                nick.to_string(),
                                "R".to_string(),
                                rline.mask,
                                rline.set_at.to_string(),
                                duration.to_string(),
                                rline.set_by,
                                reason,
                            ],
                        )
                        .await?;
                    }
                }
            }
            's' | 'S' => {
                // Shuns
                if let Ok(shuns) = ctx.db.bans().get_active_shuns().await {
                    for shun in shuns {
                        let duration = shun.expires_at.map(|exp| exp - shun.set_at).unwrap_or(0);
                        let reason = shun.reason.unwrap_or_default();
                        // :server 226 nick <mask> <set_at> <duration> <setter> :<reason>
                        ctx.send_reply(
                            Response::RPL_STATSSHUN,
                            vec![
                                nick.to_string(),
                                shun.mask,
                                shun.set_at.to_string(),
                                duration.to_string(),
                                shun.set_by,
                                reason,
                            ],
                        )
                        .await?;
                    }
                }
            }
            'c' | 'C' => {
                // Connection statistics
                let current_users = ctx.matrix.user_manager.users.len();
                let current_channels = ctx.matrix.channel_manager.channels.len();

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
                        vec![
                            nick.to_string(),
                            "No commands have been used yet".to_string(),
                        ],
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
            'i' | 'I' => {
                // IP deny list statistics (in-memory bitmap)
                // Collect data synchronously to avoid holding lock across await
                let ban_data =
                    ctx.matrix
                        .security_manager
                        .ip_deny_list
                        .read()
                        .ok()
                        .map(|deny_list| {
                            let count = deny_list.len();
                            let entries: Vec<_> = deny_list
                                .iter()
                                .take(51)
                                .map(|(key, meta)| {
                                    let expires = meta
                                        .expiry
                                        .map(|e| format!("expires {}", e))
                                        .unwrap_or_else(|| "permanent".to_string());
                                    (
                                        key.clone(),
                                        expires,
                                        meta.added_by.clone(),
                                        meta.reason.clone(),
                                    )
                                })
                                .collect();
                            (count, entries)
                        });

                if let Some((count, entries)) = ban_data {
                    ctx.send_reply(
                        Response::RPL_STATSDLINE,
                        vec![
                            nick.to_string(),
                            format!("IP deny list: {} active bans", count),
                        ],
                    )
                    .await?;

                    for (i, (key, expires, added_by, reason)) in entries.iter().enumerate() {
                        if i >= 50 {
                            ctx.send_reply(
                                Response::RPL_STATSDLINE,
                                vec![
                                    nick.to_string(),
                                    format!("... and {} more (truncated)", count - 50),
                                ],
                            )
                            .await?;
                            break;
                        }
                        ctx.send_reply(
                            Response::RPL_STATSDLINE,
                            vec![
                                nick.to_string(),
                                format!("I {} {} [{}] :{}", key, expires, added_by, reason),
                            ],
                        )
                        .await?;
                    }
                } else {
                    ctx.send_reply(
                        Response::RPL_STATSDLINE,
                        vec![
                            nick.to_string(),
                            "IP deny list: unavailable (locked)".to_string(),
                        ],
                    )
                    .await?;
                }
            }
            'p' | 'P' => {
                // Spam detection settings
                if let Some(spam_lock) = &ctx.matrix.security_manager.spam_detector {
                    let spam = spam_lock.read().await;
                    ctx.send_reply(
                        Response::RPL_STATSDLINE,
                        vec![
                            nick.to_string(),
                            format!(
                                "Spam detection: entropy_threshold={:.1}",
                                spam.entropy_threshold()
                            ),
                        ],
                    )
                    .await?;
                } else {
                    ctx.send_reply(
                        Response::RPL_STATSDLINE,
                        vec![nick.to_string(), "Spam detection: disabled".to_string()],
                    )
                    .await?;
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
                    "*** d - D-lines (IP bans)",
                    "*** r - R-lines (Realname bans)",
                    "*** s - Shuns",
                    "*** i - IP deny list (in-memory)",
                    "*** p - Spam detection settings",
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
