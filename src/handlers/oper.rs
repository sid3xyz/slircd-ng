//! Operator command handlers.
//!
//! Commands for IRC operators:
//! - OPER: Authenticate as an IRC operator
//! - KILL: Disconnect a user from the network
//! - WALLOPS: Send a message to all operators
//! - DIE: Shutdown the server (stub)
//! - REHASH: Reload server configuration (stub)

use super::{server_reply, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{Command, Message, Prefix, Response};

/// Handler for OPER command.
///
/// OPER <name> <password>
/// Authenticates a user as an IRC operator.
pub struct OperHandler;

#[async_trait]
impl Handler for OperHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        // Extract oper credentials from the message
        let (name, password) = match &msg.command {
            Command::OPER(n, p) => (n.clone(), p.clone()),
            _ => {
                // ERR_NEEDMOREPARAMS (461)
                let server_name = &ctx.matrix.config.server_name;
                let nick = {
                    let uid = ctx.uid;
                    if let Some(user_ref) = ctx.matrix.users.get(uid) {
                        let user = user_ref.read().await;
                        user.nick.clone()
                    } else {
                        "*".to_string()
                    }
                };

                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![nick, "OPER".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let server_name = &ctx.matrix.config.server_name;
        let nick = {
            let uid = ctx.uid;
            if let Some(user_ref) = ctx.matrix.users.get(uid) {
                let user = user_ref.read().await;
                user.nick.clone()
            } else {
                "*".to_string()
            }
        };

        // Check oper blocks in config
        let oper_block = ctx.matrix.config.oper_blocks.iter().find(|block| block.name == name);

        let Some(oper_block) = oper_block else {
            // ERR_PASSWDMISMATCH (464)
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
        let password_valid = oper_block.password == password;

        if !password_valid {
            // ERR_PASSWDMISMATCH (464)
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

        // RPL_YOUREOPER (381)
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
/// KILL <nickname> :<reason>
/// Disconnects a user from the network. Requires operator privileges.
pub struct KillHandler;

#[async_trait]
impl Handler for KillHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        // Extract target and reason from the message
        let (target_nick, reason) = match &msg.command {
            Command::KILL(target, reason) => (target.clone(), reason.clone()),
            _ => {
                // ERR_NEEDMOREPARAMS (461)
                let server_name = &ctx.matrix.config.server_name;
                let nick = {
                    if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                        let user = user_ref.read().await;
                        user.nick.clone()
                    } else {
                        "*".to_string()
                    }
                };

                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![nick, "KILL".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let server_name = &ctx.matrix.config.server_name;
        
        // Get killer info
        let (killer_nick, killer_user, killer_host, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.user.clone(), user.host.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            // ERR_NOPRIVILEGES (481)
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![killer_nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Find target user
        use slirc_proto::irc_to_lower;
        let target_lower = irc_to_lower(&target_nick);
        let target_uid = ctx.matrix.nicks.get(&target_lower).map(|r| r.value().clone());

        let Some(target_uid) = target_uid else {
            // ERR_NOSUCHNICK (401)
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHNICK,
                vec![killer_nick, target_nick, "No such nick/channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Get target's sender to send KILL message
        // Note: In a real implementation, we'd need access to the target's sender
        // For now, we just remove them from the matrix state

        // Build KILL message
        let kill_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                killer_nick.clone(),
                killer_user,
                killer_host,
            )),
            command: Command::Raw("KILL".to_string(), vec![
                target_nick.clone(),
                format!("Killed by {} ({})", killer_nick, reason),
            ]),
        };

        // TODO: Send KILL to target client and disconnect them
        // This requires access to the router or connection manager
        // For now, we log the action

        tracing::info!(
            killer = %killer_nick,
            target = %target_nick,
            reason = %reason,
            "KILL command executed"
        );

        // Remove target from all channels and broadcast QUIT
        let target_channels: Vec<String> = {
            if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
                let user = user_ref.read().await;
                user.channels.iter().cloned().collect()
            } else {
                vec![]
            }
        };

        let quit_reason = format!("Killed by {} ({})", killer_nick, reason);
        
        // Build QUIT message for target
        let quit_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                target_nick.clone(),
                "user".to_string(),
                "host".to_string(),
            )),
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
/// WALLOPS :<message>
/// Sends a message to all users with +w mode (operators).
pub struct WallopsHandler;

#[async_trait]
impl Handler for WallopsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        // Extract message from the command
        let wallops_text = match &msg.command {
            Command::WALLOPS(text) => text.clone(),
            _ => {
                // ERR_NEEDMOREPARAMS (461)
                let server_name = &ctx.matrix.config.server_name;
                let nick = {
                    if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                        let user = user_ref.read().await;
                        user.nick.clone()
                    } else {
                        "*".to_string()
                    }
                };

                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![nick, "WALLOPS".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let server_name = &ctx.matrix.config.server_name;
        
        // Get sender info
        let (sender_nick, sender_user, sender_host, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.user.clone(), user.host.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            // ERR_NOPRIVILEGES (481)
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![sender_nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build WALLOPS message
        let wallops_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                sender_nick.clone(),
                sender_user,
                sender_host,
            )),
            command: Command::WALLOPS(wallops_text),
        };

        // Send to all users with +w mode (wallops)
        for user_entry in ctx.matrix.users.iter() {
            let user = user_entry.read().await;
            // Check if user has +w mode (wallops) or is an operator
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
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &Message) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;
        
        // Get user info
        let (nick, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            // ERR_NOPRIVILEGES (481)
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // TODO: Implement actual shutdown
        // For now, just send a notice
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
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &Message) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;
        
        // Get user info
        let (nick, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            // ERR_NOPRIVILEGES (481)
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // RPL_REHASHING (382)
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
