//! Operator command handlers.
//!
//! Commands for IRC operators:
//! - OPER: Authenticate as an IRC operator
//! - KILL: Disconnect a user from the network
//! - WALLOPS: Send a message to all operators
//! - DIE: Shutdown the server (stub)
//! - REHASH: Reload server configuration (stub)

use super::{err_needmoreparams, err_noprivileges, err_nosuchnick, server_reply, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, MessageRef, Prefix, Response};

/// Get user's nick, falling back to "*" if not found.
async fn get_nick_or_star(ctx: &Context<'_>) -> String {
    if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
        user_ref.read().await.nick.clone()
    } else {
        "*".to_string()
    }
}

/// Get user's nick and oper status. Returns None if user not found.
async fn get_oper_info(ctx: &Context<'_>) -> Option<(String, bool)> {
    let user_ref = ctx.matrix.users.get(ctx.uid)?;
    let user = user_ref.read().await;
    Some((user.nick.clone(), user.modes.oper))
}

/// Get full user info for message construction.
async fn get_user_full_info(ctx: &Context<'_>) -> Option<(String, String, String, bool)> {
    let user_ref = ctx.matrix.users.get(ctx.uid)?;
    let user = user_ref.read().await;
    Some((user.nick.clone(), user.user.clone(), user.host.clone(), user.modes.oper))
}

/// Resolve a nick to UID. Returns None if not found.
fn resolve_nick(ctx: &Context<'_>, nick: &str) -> Option<String> {
    let lower = irc_to_lower(nick);
    ctx.matrix.nicks.get(&lower).map(|r| r.value().clone())
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
        let server_name = &ctx.matrix.config.server_name;

        // OPER <name> <password>
        let name = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender.send(err_needmoreparams(server_name, &nick, "OPER")).await?;
                return Ok(());
            }
        };
        let password = match msg.arg(1) {
            Some(p) if !p.is_empty() => p,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender.send(err_needmoreparams(server_name, &nick, "OPER")).await?;
                return Ok(());
            }
        };

        let nick = get_nick_or_star(ctx).await;

        // Check oper blocks in config
        let oper_block = ctx.matrix.config.oper_blocks.iter().find(|block| block.name == name);

        let Some(oper_block) = oper_block else {
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
            let reply = server_reply(
                server_name,
                Response::ERR_PASSWDMISMATCH,
                vec![nick, "Password incorrect".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // TODO: Check hostmask if specified in oper block
        // For now, grant operator status

        // Set +o mode on user
        if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
            let mut user = user_ref.write().await;
            user.modes.oper = true;
        }

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
        let server_name = &ctx.matrix.config.server_name;

        // KILL <target> <reason>
        let target_nick = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender.send(err_needmoreparams(server_name, &nick, "KILL")).await?;
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
            ctx.sender.send(err_noprivileges(server_name, &killer_nick)).await?;
            return Ok(());
        }

        // Find target user
        let Some(target_uid) = resolve_nick(ctx, target_nick) else {
            ctx.sender.send(err_nosuchnick(server_name, &killer_nick, target_nick)).await?;
            return Ok(());
        };

        // Build KILL message
        let kill_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(killer_nick.clone(), killer_user, killer_host)),
            command: Command::KILL(target_nick.to_string(), format!("Killed by {killer_nick} ({reason})")),
        };

        tracing::info!(killer = %killer_nick, target = %target_nick, reason = %reason, "KILL command executed");

        // Remove target from all channels and broadcast QUIT
        let target_channels: Vec<String> = {
            if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
                let user = user_ref.read().await;
                user.channels.iter().cloned().collect()
            } else {
                vec![]
            }
        };

        let quit_reason = format!("Killed by {killer_nick} ({reason})");

        // Build QUIT message for target
        let quit_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(target_nick.to_string(), "user".to_string(), "host".to_string())),
            command: Command::QUIT(Some(quit_reason)),
        };

        // Remove from channels and broadcast QUIT
        for channel_name in target_channels {
            if let Some(channel_ref) = ctx.matrix.channels.get(&channel_name) {
                let mut channel = channel_ref.write().await;
                channel.members.remove(&target_uid);

                // Broadcast QUIT to remaining channel members
                for member_uid in channel.members.keys() {
                    if let Some(member_ref) = ctx.matrix.users.get(member_uid) {
                        let member = member_ref.read().await;
                        if let Some(sender) = ctx.matrix.senders.get(&member.uid) {
                            let _ = sender.send(quit_msg.clone()).await;
                        }
                    }
                }
            }
        }

        // Remove user from matrix state
        if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
            let user = user_ref.read().await;
            let nick_lower = irc_to_lower(&user.nick);
            ctx.matrix.nicks.remove(&nick_lower);
        }
        ctx.matrix.users.remove(&target_uid);

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
        let server_name = &ctx.matrix.config.server_name;

        // WALLOPS <message>
        let wallops_text = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                ctx.sender.send(err_needmoreparams(server_name, &nick, "WALLOPS")).await?;
                return Ok(());
            }
        };

        // Get sender info
        let Some((sender_nick, sender_user, sender_host, is_oper)) = get_user_full_info(ctx).await
        else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, &sender_nick)).await?;
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
        let server_name = &ctx.matrix.config.server_name;

        let Some((nick, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, &nick)).await?;
            return Ok(());
        }

        // TODO: Implement actual shutdown
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(nick.clone(), "DIE command received (not implemented - use process signals)".to_string()),
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
        let server_name = &ctx.matrix.config.server_name;

        let Some((nick, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            ctx.sender.send(err_noprivileges(server_name, &nick)).await?;
            return Ok(());
        }

        let reply = server_reply(
            server_name,
            Response::RPL_REHASHING,
            vec![nick.clone(), "config.toml".to_string(), "Rehashing".to_string()],
        );
        ctx.sender.send(reply).await?;

        // TODO: Implement actual config reload
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(nick.clone(), "REHASH acknowledged (config reload not yet implemented)".to_string()),
        };
        ctx.sender.send(notice).await?;

        tracing::info!(oper = %nick, "REHASH command received (stub)");

        Ok(())
    }
}
