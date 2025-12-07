//! MONITOR command handler (IRCv3).
//!
//! Implements presence notification via MONITOR.
//! Reference: <https://ircv3.net/specs/extensions/monitor>

use super::{Context, Handler, HandlerResult, server_reply, with_label};
use async_trait::async_trait;
use dashmap::DashSet;
use slirc_proto::{MessageRef, Response, irc_to_lower};
use tracing::debug;

/// Maximum number of nicknames a user can monitor.
const MAX_MONITOR_TARGETS: usize = 100;

/// Handler for MONITOR command.
///
/// `MONITOR + targets` - Add targets to monitor list
/// `MONITOR - targets` - Remove targets from monitor list
/// `MONITOR C` - Clear monitor list
/// `MONITOR L` - List monitored targets
/// `MONITOR S` - Show status of monitored targets
pub struct MonitorHandler;

#[async_trait]
impl Handler for MonitorHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .clone()
            .unwrap_or_else(|| "*".to_string());

        // MONITOR <+/-/C/L/S> [targets]
        let subcommand = match msg.arg(0) {
            Some(s) if !s.is_empty() => s,
            _ => {
                // No subcommand - send usage hint
                return Ok(());
            }
        };

        match subcommand {
            "+" => handle_add(ctx, msg, &nick, server_name).await,
            "-" => handle_remove(ctx, msg, &nick).await,
            "C" | "c" => handle_clear(ctx),
            "L" | "l" => handle_list(ctx, &nick, server_name).await,
            "S" | "s" => handle_status(ctx, &nick, server_name).await,
            _ => {
                debug!(subcommand = %subcommand, "Unknown MONITOR subcommand");
                Ok(())
            }
        }
    }
}

/// Handle MONITOR + targets - add nicknames to monitor list.
async fn handle_add(
    ctx: &mut Context<'_>,
    msg: &MessageRef<'_>,
    nick: &str,
    server_name: &str,
) -> HandlerResult {
    let targets = match msg.arg(1) {
        Some(t) if !t.is_empty() => t,
        _ => return Ok(()),
    };

    // Get or create this user's monitor set
    let user_monitors = ctx
        .matrix
        .monitors
        .entry(ctx.uid.to_string())
        .or_insert_with(DashSet::new);

    let mut online = Vec::new();
    let mut offline = Vec::new();

    for target in targets.split(',') {
        let target = target.trim();
        if target.is_empty() {
            continue;
        }

        let target_lower = irc_to_lower(target);

        // Check if we're at the limit
        if user_monitors.len() >= MAX_MONITOR_TARGETS {
            // Send ERR_MONLISTFULL (734)
            let reply = server_reply(
                server_name,
                Response::ERR_MONLISTFULL,
                vec![
                    nick.to_string(),
                    MAX_MONITOR_TARGETS.to_string(),
                    target.to_string(),
                    "Monitor list is full".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            break;
        }

        // Add to this user's monitor set
        user_monitors.insert(target_lower.clone());

        // Add to reverse mapping (who is monitoring this nick)
        ctx.matrix
            .monitoring
            .entry(target_lower.clone())
            .or_insert_with(DashSet::new)
            .insert(ctx.uid.to_string());

        // Check if target is online
        if let Some(target_uid) = ctx.matrix.nicks.get(&target_lower) {
            if let Some(user_ref) = ctx.matrix.users.get(target_uid.value()) {
                let user = user_ref.read().await;
                online.push(format!("{}!{}@{}", user.nick, user.user, user.visible_host));
            }
        } else {
            offline.push(target.to_string());
        }
    }

    // Send RPL_MONONLINE (730) for online targets
    if !online.is_empty() {
        let reply = server_reply(
            server_name,
            Response::RPL_MONONLINE,
            vec![nick.to_string(), online.join(",")],
        );
        ctx.sender.send(reply).await?;
    }

    // Send RPL_MONOFFLINE (731) for offline targets
    if !offline.is_empty() {
        let reply = server_reply(
            server_name,
            Response::RPL_MONOFFLINE,
            vec![nick.to_string(), offline.join(",")],
        );
        ctx.sender.send(reply).await?;
    }

    Ok(())
}

/// Handle MONITOR - targets - remove nicknames from monitor list.
async fn handle_remove(ctx: &mut Context<'_>, msg: &MessageRef<'_>, _nick: &str) -> HandlerResult {
    let targets = match msg.arg(1) {
        Some(t) if !t.is_empty() => t,
        _ => return Ok(()),
    };

    if let Some(user_monitors) = ctx.matrix.monitors.get(ctx.uid) {
        for target in targets.split(',') {
            let target = target.trim();
            if target.is_empty() {
                continue;
            }

            let target_lower = irc_to_lower(target);

            // Remove from this user's monitor set
            user_monitors.remove(&target_lower);

            // Remove from reverse mapping
            if let Some(watchers) = ctx.matrix.monitoring.get(&target_lower) {
                watchers.remove(ctx.uid);
            }
        }
    }

    Ok(())
}

/// Handle MONITOR C - clear all monitored nicknames.
#[allow(clippy::result_large_err)]
fn handle_clear(ctx: &mut Context<'_>) -> HandlerResult {
    if let Some((_, user_monitors)) = ctx.matrix.monitors.remove(ctx.uid) {
        // Remove from all reverse mappings
        for target_lower in user_monitors.iter() {
            if let Some(watchers) = ctx.matrix.monitoring.get(target_lower.as_str()) {
                watchers.remove(ctx.uid);
            }
        }
    }

    Ok(())
}

/// Handle MONITOR L - list all monitored nicknames.
async fn handle_list(ctx: &mut Context<'_>, nick: &str, server_name: &str) -> HandlerResult {
    if let Some(user_monitors) = ctx.matrix.monitors.get(ctx.uid) {
        // Collect all monitored nicks
        let targets: Vec<String> = user_monitors.iter().map(|r| r.clone()).collect();

        // Send in batches to avoid line length limits
        for chunk in targets.chunks(10) {
            let reply = server_reply(
                server_name,
                Response::RPL_MONLIST,
                vec![nick.to_string(), chunk.join(",")],
            );
            ctx.sender.send(reply).await?;
        }
    }

    // Send end of list with label
    let reply = with_label(
        server_reply(
            server_name,
            Response::RPL_ENDOFMONLIST,
            vec![nick.to_string(), "End of MONITOR list".to_string()],
        ),
        ctx.label.as_deref(),
    );
    ctx.sender.send(reply).await?;

    Ok(())
}

/// Handle MONITOR S - show status of all monitored nicknames.
async fn handle_status(ctx: &mut Context<'_>, nick: &str, server_name: &str) -> HandlerResult {
    let mut online = Vec::new();
    let mut offline = Vec::new();

    if let Some(user_monitors) = ctx.matrix.monitors.get(ctx.uid) {
        for target_lower in user_monitors.iter() {
            if let Some(target_uid) = ctx.matrix.nicks.get(target_lower.as_str()) {
                if let Some(user_ref) = ctx.matrix.users.get(target_uid.value()) {
                    let user = user_ref.read().await;
                    online.push(format!("{}!{}@{}", user.nick, user.user, user.visible_host));
                }
            } else {
                offline.push(target_lower.clone());
            }
        }
    }

    // Send RPL_MONONLINE (730) for online targets
    if !online.is_empty() {
        for chunk in online.chunks(5) {
            let reply = server_reply(
                server_name,
                Response::RPL_MONONLINE,
                vec![nick.to_string(), chunk.join(",")],
            );
            ctx.sender.send(reply).await?;
        }
    }

    // Send RPL_MONOFFLINE (731) for offline targets
    if !offline.is_empty() {
        for chunk in offline.chunks(10) {
            let reply = server_reply(
                server_name,
                Response::RPL_MONOFFLINE,
                vec![nick.to_string(), chunk.join(",")],
            );
            ctx.sender.send(reply).await?;
        }
    }

    Ok(())
}

// ============================================================================
// MONITOR notification helpers (called from connection handlers)
// ============================================================================

use crate::state::Matrix;
use std::sync::Arc;

/// Notify all monitors that a user has come online.
///
/// Called after successful registration (NICK + USER complete).
pub async fn notify_monitors_online(matrix: &Arc<Matrix>, nick: &str, user: &str, host: &str) {
    let nick_lower = irc_to_lower(nick);
    let server_name = &matrix.server_info.name;

    if let Some(watchers) = matrix.monitoring.get(&nick_lower) {
        let hostmask = format!("{}!{}@{}", nick, user, host);
        let reply = server_reply(
            server_name,
            Response::RPL_MONONLINE,
            vec!["*".to_string(), hostmask],
        );

        for watcher_uid in watchers.iter() {
            if let Some(sender) = matrix.senders.get(watcher_uid.as_str()) {
                let _ = sender.send(reply.clone()).await;
            }
        }
    }
}

/// Notify all monitors that a user has gone offline.
///
/// Called when a user disconnects or changes nick.
pub async fn notify_monitors_offline(matrix: &Arc<Matrix>, nick: &str) {
    let nick_lower = irc_to_lower(nick);
    let server_name = &matrix.server_info.name;

    if let Some(watchers) = matrix.monitoring.get(&nick_lower) {
        let reply = server_reply(
            server_name,
            Response::RPL_MONOFFLINE,
            vec!["*".to_string(), nick.to_string()],
        );

        for watcher_uid in watchers.iter() {
            if let Some(sender) = matrix.senders.get(watcher_uid.as_str()) {
                let _ = sender.send(reply.clone()).await;
            }
        }
    }
}

/// Clean up a user's monitor entries when they disconnect.
pub fn cleanup_monitors(matrix: &Arc<Matrix>, uid: &str) {
    if let Some((_, user_monitors)) = matrix.monitors.remove(uid) {
        // Remove from all reverse mappings
        for target_lower in user_monitors.iter() {
            if let Some(watchers) = matrix.monitoring.get(target_lower.as_str()) {
                watchers.remove(uid);
            }
        }
    }
}
