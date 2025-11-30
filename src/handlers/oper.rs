//! Operator command handlers.
//!
//! Commands for IRC operators:
//! - OPER: Authenticate as an IRC operator
//! - KILL: Disconnect a user from the network
//! - WALLOPS: Send a message to all operators
//! - DIE: Shutdown the server (stub)
//! - REHASH: Reload server configuration (stub)

use super::{
    Context, Handler, HandlerResult, err_needmoreparams, err_noprivileges, err_nosuchnick,
    get_nick_or_star, matches_hostmask, require_oper, resolve_nick_to_uid, server_reply,
};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

/// Get full user info for message construction.
async fn get_user_full_info(ctx: &Context<'_>) -> Option<(String, String, String, bool)> {
    let user_ref = ctx.matrix.users.get(ctx.uid)?;
    let user = user_ref.read().await;
    Some((
        user.nick.clone(),
        user.user.clone(),
        user.host.clone(),
        user.modes.oper,
    ))
}

/// Handler for OPER command.
///
/// `OPER name password`
///
/// Authenticates a user as an IRC operator.
pub struct OperHandler;

#[async_trait]
impl Handler for OperHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // OPER <name> <password>
        let name = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "OPER"))
                    .await?;
                return Ok(());
            }
        };
        let password = match msg.arg(1) {
            Some(p) if !p.is_empty() => p,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "OPER"))
                    .await?;
                return Ok(());
            }
        };

        let nick = get_nick_or_star(ctx).await;

        // Brute-force protection: Check rate limiting and attempt counter
        const MAX_OPER_ATTEMPTS: u8 = 3;
        const OPER_DELAY_MS: u64 = 3000; // 3 seconds between attempts (anti-brute-force)
        const LOCKOUT_DELAY_MS: u64 = 30000; // 30 second lockout after max attempts

        let now = std::time::Instant::now();

        // Check if user is in lockout period
        if ctx.handshake.failed_oper_attempts >= MAX_OPER_ATTEMPTS
            && let Some(last_attempt) = ctx.handshake.last_oper_attempt
        {
            let elapsed = now.duration_since(last_attempt).as_millis() as u64;
            if elapsed < LOCKOUT_DELAY_MS {
                let remaining_sec = (LOCKOUT_DELAY_MS - elapsed) / 1000;
                let reply = server_reply(
                    server_name,
                    Response::ERR_PASSWDMISMATCH,
                    vec![
                        nick.clone(),
                        format!(
                            "Too many failed attempts. Try again in {} seconds.",
                            remaining_sec
                        ),
                    ],
                );
                ctx.sender.send(reply).await?;
                tracing::warn!(nick = %nick, attempts = ctx.handshake.failed_oper_attempts, "OPER brute-force lockout active");
                return Ok(());
            } else {
                // Lockout period expired, reset counter
                ctx.handshake.failed_oper_attempts = 0;
            }
        }

        // Enforce delay between attempts
        if let Some(last_attempt) = ctx.handshake.last_oper_attempt {
            let elapsed = now.duration_since(last_attempt).as_millis() as u64;
            if elapsed < OPER_DELAY_MS {
                let remaining_ms = OPER_DELAY_MS - elapsed;
                tokio::time::sleep(tokio::time::Duration::from_millis(remaining_ms)).await;
            }
        }

        ctx.handshake.last_oper_attempt = Some(now);

        // Check oper blocks in config
        let oper_block = ctx
            .matrix
            .config
            .oper_blocks
            .iter()
            .find(|block| block.name == name);

        let Some(oper_block) = oper_block else {
            ctx.handshake.failed_oper_attempts += 1;
            tracing::warn!(
                nick = %nick,
                oper_name = %name,
                attempts = ctx.handshake.failed_oper_attempts,
                "OPER failed: unknown oper name"
            );
            let reply = server_reply(
                server_name,
                Response::ERR_PASSWDMISMATCH,
                vec![nick, "Password incorrect".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Validate password
        // TODO: Support bcrypt hashes for production
        if oper_block.password != password {
            ctx.handshake.failed_oper_attempts += 1;
            tracing::warn!(
                nick = %nick,
                oper_name = %name,
                attempts = ctx.handshake.failed_oper_attempts,
                "OPER failed: incorrect password"
            );
            let reply = server_reply(
                server_name,
                Response::ERR_PASSWDMISMATCH,
                vec![nick, "Password incorrect".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Check hostmask if specified in oper block
        if let Some(ref required_mask) = oper_block.hostmask {
            // Build user's actual hostmask (nick!user@host)
            let (user_nick, user_user, user_host) = if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.user.clone(), user.host.clone())
            } else {
                // Fallback to handshake data for pre-registration
                let hs_nick = ctx.handshake.nick.clone().unwrap_or_else(|| nick.clone());
                let hs_user = ctx.handshake.user.clone().unwrap_or_else(|| "unknown".to_string());
                (hs_nick, hs_user, ctx.remote_addr.ip().to_string())
            };
            let user_mask = format!("{}!{}@{}", user_nick, user_user, user_host);

            if !matches_hostmask(required_mask, &user_mask) {
                ctx.handshake.failed_oper_attempts += 1;
                tracing::warn!(
                    nick = %nick,
                    oper_name = %name,
                    user_mask = %user_mask,
                    required_mask = %required_mask,
                    attempts = ctx.handshake.failed_oper_attempts,
                    "OPER failed: hostmask mismatch"
                );
                let reply = server_reply(
                    server_name,
                    Response::ERR_NOOPERHOST,
                    vec![nick, "No O-lines for your host".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        // Success - reset attempt counter
        ctx.handshake.failed_oper_attempts = 0;

        // Set +o mode on user
        if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let mut user = user_ref.write().await;
            user.modes.oper = true;
        }

        tracing::info!(nick = %nick, oper_name = %name, "OPER successful");

        let reply = server_reply(
            server_name,
            Response::RPL_YOUREOPER,
            vec![nick, "You are now an IRC operator".to_string()],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for KILL command.
///
/// `KILL nickname :reason`
///
/// Disconnects a user from the network. Requires operator privileges.
pub struct KillHandler;

#[async_trait]
impl Handler for KillHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // KILL <target> <reason>
        let target_nick = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "KILL"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Get killer info
        let Some((killer_nick, killer_user, killer_host, is_oper)) = get_user_full_info(ctx).await
        else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender
                .send(err_noprivileges(server_name, &killer_nick))
                .await?;
            return Ok(());
        }

        // Find target user
        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            ctx.sender
                .send(err_nosuchnick(server_name, &killer_nick, target_nick))
                .await?;
            return Ok(());
        };

        // Build KILL message for confirmation
        let kill_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                killer_nick.clone(),
                killer_user,
                killer_host,
            )),
            command: Command::KILL(
                target_nick.to_string(),
                format!("Killed by {killer_nick} ({reason})"),
            ),
        };

        tracing::info!(killer = %killer_nick, target = %target_nick, reason = %reason, "KILL command executed");

        // Use centralized disconnect logic
        let quit_reason = format!("Killed by {killer_nick} ({reason})");
        ctx.matrix.disconnect_user(&target_uid, &quit_reason).await;

        // Send confirmation to killer
        let _ = ctx.sender.send(kill_msg).await;

        Ok(())
    }
}

/// Handler for WALLOPS command.
///
/// `WALLOPS :message`
///
/// Sends a message to all users with +w mode (operators).
pub struct WallopsHandler;

#[async_trait]
impl Handler for WallopsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // WALLOPS <message>
        let wallops_text = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "WALLOPS"))
                    .await?;
                return Ok(());
            }
        };

        // Get sender info
        let Some((sender_nick, sender_user, sender_host, is_oper)) = get_user_full_info(ctx).await
        else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender
                .send(err_noprivileges(server_name, &sender_nick))
                .await?;
            return Ok(());
        }

        // Build WALLOPS message
        let wallops_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(sender_nick, sender_user, sender_host)),
            command: Command::WALLOPS(wallops_text.to_string()),
        };

        // Send to all users with +w mode (wallops) or operators
        for user_entry in ctx.matrix.users.iter() {
            let user = user_entry.read().await;
            if (user.modes.wallops || user.modes.oper)
                && let Some(sender) = ctx.matrix.senders.get(&user.uid)
            {
                let _ = sender.send(wallops_msg.clone()).await;
            }
        }

        Ok(())
    }
}

/// Handler for DIE command (stub).
///
/// DIE
/// Shuts down the server. Requires operator privileges.
pub struct DieHandler;

#[async_trait]
impl Handler for DieHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // TODO: Implement actual shutdown
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                nick.clone(),
                "DIE command received (not implemented - use process signals)".to_string(),
            ),
        };
        ctx.sender.send(notice).await?;

        tracing::warn!(oper = %nick, "DIE command received (stub)");

        Ok(())
    }
}

/// Handler for REHASH command (stub).
///
/// REHASH
/// Reloads server configuration. Requires operator privileges.
pub struct RehashHandler;

#[async_trait]
impl Handler for RehashHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        let reply = server_reply(
            server_name,
            Response::RPL_REHASHING,
            vec![
                nick.clone(),
                "config.toml".to_string(),
                "Rehashing".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        // TODO: Implement actual config reload
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                nick.clone(),
                "REHASH acknowledged (config reload not yet implemented)".to_string(),
            ),
        };
        ctx.sender.send(notice).await?;

        tracing::info!(oper = %nick, "REHASH command received (stub)");

        Ok(())
    }
}
