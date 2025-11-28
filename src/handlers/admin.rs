//! Admin command handlers (SA* commands).
//!
//! Server admin commands (operator-only):
//! - SAJOIN: Force a user to join a channel
//! - SAPART: Force a user to leave a channel
//! - SAMODE: Set channel modes as server
//! - SANICK: Force a user to change nick

use super::{server_reply, Context, Handler, HandlerResult};
use crate::state::MemberModes;
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, Prefix, Response};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Handler for SAJOIN command.
///
/// SAJOIN <nick> <channel>
/// Forces a user to join a channel.
pub struct SajoinHandler;

#[async_trait]
impl Handler for SajoinHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;
        
        // Get operator info
        let (oper_nick, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![oper_nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Extract target nick and channel
        let (target_nick, channel_name) = match &msg.command {
            Command::SAJOIN(nick, channel) => (nick.clone(), channel.clone()),
            _ => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![oper_nick, "SAJOIN".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Find target user
        let target_lower = irc_to_lower(&target_nick);
        let target_uid = ctx.matrix.nicks.get(&target_lower).map(|r| r.value().clone());

        let Some(target_uid) = target_uid else {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHNICK,
                vec![oper_nick, target_nick, "No such nick/channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Validate channel name
        if !channel_name.starts_with('#') && !channel_name.starts_with('&') {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHCHANNEL,
                vec![oper_nick, channel_name, "Invalid channel name".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        let channel_lower = irc_to_lower(&channel_name);

        // Get or create channel
        let channel_ref = ctx.matrix.channels
            .entry(channel_lower.clone())
            .or_insert_with(|| Arc::new(RwLock::new(crate::state::Channel::new(channel_name.clone()))))
            .clone();

        // Get target user info for JOIN message
        let (target_user, target_host, target_realname) = {
            if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
                let user = user_ref.read().await;
                (user.user.clone(), user.host.clone(), user.nick.clone())
            } else {
                return Ok(());
            }
        };

        // Add target to channel
        {
            let mut channel = channel_ref.write().await;
            if !channel.is_member(&target_uid) {
                channel.add_member(target_uid.clone(), MemberModes::default());
            }
        }

        // Add channel to user's list
        if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
            let mut user = user_ref.write().await;
            user.channels.insert(channel_lower.clone());
        }

        // Build JOIN message
        let join_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                target_realname,
                target_user,
                target_host,
            )),
            command: Command::JOIN(channel_name.clone(), None, None),
        };

        // Broadcast to channel
        ctx.matrix.broadcast_to_channel(&channel_lower, join_msg, None).await;

        tracing::info!(
            oper = %oper_nick,
            target = %target_nick,
            channel = %channel_name,
            "SAJOIN: Forced user to join channel"
        );

        // Confirm to operator
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                oper_nick,
                format!("SAJOIN: {} has been forced to join {}", target_nick, channel_name),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for SAPART command.
///
/// SAPART <nick> <channel> [reason]
/// Forces a user to leave a channel.
pub struct SapartHandler;

#[async_trait]
impl Handler for SapartHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;
        
        // Get operator info
        let (oper_nick, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![oper_nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Extract target nick, channel, and optional reason
        let (target_nick, channel_name, reason) = match &msg.command {
            Command::SAPART(nick, channel) => (nick.clone(), channel.clone(), None),
            _ => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![oper_nick, "SAPART".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Find target user
        let target_lower = irc_to_lower(&target_nick);
        let target_uid = ctx.matrix.nicks.get(&target_lower).map(|r| r.value().clone());

        let Some(target_uid) = target_uid else {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHNICK,
                vec![oper_nick, target_nick, "No such nick/channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        let channel_lower = irc_to_lower(&channel_name);

        // Check if channel exists
        let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) else {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHCHANNEL,
                vec![oper_nick, channel_name, "No such channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Get target user info for PART message
        let (target_user, target_host, target_realname) = {
            if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
                let user = user_ref.read().await;
                (user.user.clone(), user.host.clone(), user.nick.clone())
            } else {
                return Ok(());
            }
        };

        // Build PART message
        let part_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                target_realname,
                target_user,
                target_host,
            )),
            command: Command::PART(channel_name.clone(), reason.clone()),
        };

        // Broadcast PART to channel (before removing member)
        ctx.matrix.broadcast_to_channel(&channel_lower, part_msg, None).await;

        // Remove from channel
        {
            let channel = channel_ref.clone();
            let mut channel = channel.write().await;
            channel.remove_member(&target_uid);
        }

        // Remove channel from user's list
        if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
            let mut user = user_ref.write().await;
            user.channels.remove(&channel_lower);
        }

        tracing::info!(
            oper = %oper_nick,
            target = %target_nick,
            channel = %channel_name,
            reason = ?reason,
            "SAPART: Forced user to leave channel"
        );

        // Confirm to operator
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                oper_nick,
                format!("SAPART: {} has been forced to leave {}", target_nick, channel_name),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for SANICK command.
///
/// SANICK <oldnick> <newnick>
/// Forces a user to change their nickname.
pub struct SanickHandler;

#[async_trait]
impl Handler for SanickHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;
        
        // Get operator info
        let (oper_nick, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![oper_nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Extract old and new nicks
        let (old_nick, new_nick) = match &msg.command {
            Command::SANICK(old, new) => (old.clone(), new.clone()),
            _ => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![oper_nick, "SANICK".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Find target user
        let old_lower = irc_to_lower(&old_nick);
        let target_uid = ctx.matrix.nicks.get(&old_lower).map(|r| r.value().clone());

        let Some(target_uid) = target_uid else {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHNICK,
                vec![oper_nick, old_nick, "No such nick/channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        // Check if new nick is already in use
        let new_lower = irc_to_lower(&new_nick);
        if ctx.matrix.nicks.contains_key(&new_lower) {
            let reply = server_reply(
                server_name,
                Response::ERR_NICKNAMEINUSE,
                vec![oper_nick, new_nick, "Nickname is already in use".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Get target user info for NICK message
        let (target_user, target_host) = {
            if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
                let user = user_ref.read().await;
                (user.user.clone(), user.host.clone())
            } else {
                return Ok(());
            }
        };

        // Build NICK message
        let nick_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(
                old_nick.clone(),
                target_user,
                target_host,
            )),
            command: Command::NICK(new_nick.clone()),
        };

        // Update nick mapping
        ctx.matrix.nicks.remove(&old_lower);
        ctx.matrix.nicks.insert(new_lower, target_uid.clone());

        // Update user's nick
        if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
            let mut user = user_ref.write().await;
            user.nick = new_nick.clone();
        }

        // Broadcast NICK change to all channels the user is in
        if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
            let user = user_ref.read().await;
            for channel_name in &user.channels {
                ctx.matrix.broadcast_to_channel(channel_name, nick_msg.clone(), None).await;
            }
        }

        // Also send to the target user
        if let Some(sender) = ctx.matrix.senders.get(&target_uid) {
            let _ = sender.send(nick_msg).await;
        }

        tracing::info!(
            oper = %oper_nick,
            old_nick = %old_nick,
            new_nick = %new_nick,
            "SANICK: Forced nick change"
        );

        // Confirm to operator
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                oper_nick,
                format!("SANICK: {} has been forced to change nick to {}", old_nick, new_nick),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for SAMODE command (stub).
///
/// SAMODE <channel> <modes> [params]
/// Sets channel modes as the server (bypassing op requirement).
pub struct SamodeHandler;

#[async_trait]
impl Handler for SamodeHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &Message) -> HandlerResult {
        let server_name = &ctx.matrix.config.server_name;
        
        // Get operator info
        let (oper_nick, is_oper) = {
            if let Some(user_ref) = ctx.matrix.users.get(ctx.uid) {
                let user = user_ref.read().await;
                (user.nick.clone(), user.modes.oper)
            } else {
                return Ok(());
            }
        };

        // Check if user is an operator
        if !is_oper {
            let reply = server_reply(
                server_name,
                Response::ERR_NOPRIVILEGES,
                vec![oper_nick, "Permission Denied - You're not an IRC operator".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Extract channel and modes
        let (channel_name, mode_str) = match &msg.command {
            Command::SAMODE(target, modes, params) => {
                let mode_with_params = if let Some(p) = params {
                    format!("{} {}", modes, p)
                } else {
                    modes.clone()
                };
                (target.clone(), mode_with_params)
            }
            _ => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![oper_nick, "SAMODE".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let channel_lower = irc_to_lower(&channel_name);

        // Check if channel exists
        if !ctx.matrix.channels.contains_key(&channel_lower) {
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHCHANNEL,
                vec![oper_nick, channel_name, "No such channel".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // TODO: Parse and apply modes
        // For now, just broadcast the MODE message as if from server

        let mode_msg = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::Raw("MODE".to_string(), vec![channel_name.clone(), mode_str.clone()]),
        };

        ctx.matrix.broadcast_to_channel(&channel_lower, mode_msg, None).await;

        tracing::info!(
            oper = %oper_nick,
            channel = %channel_name,
            modes = %mode_str,
            "SAMODE: Server mode change"
        );

        // Confirm to operator
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                oper_nick,
                format!("SAMODE: {} {}", channel_name, mode_str),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}
