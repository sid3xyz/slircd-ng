//! Admin command handlers (SA* commands).
//!
//! Server admin commands (operator-only):
//! - SAJOIN: Force a user to join a channel
//! - SAPART: Force a user to leave a channel
//! - SAMODE: Set channel modes as server
//! - SANICK: Force a user to change nick

use super::{
    apply_channel_modes_typed, err_needmoreparams, err_nosuchchannel,
    err_nosuchnick, require_oper, resolve_nick_to_uid, server_reply, Context, Handler, HandlerResult,
};
use crate::state::MemberModes;
use async_trait::async_trait;
use slirc_proto::{irc_to_lower, Command, Message, MessageRef, Mode, Prefix, Response};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Get user prefix info (user, host, nick) for message construction.
async fn get_user_prefix(ctx: &Context<'_>, uid: &str) -> Option<(String, String, String)> {
    let user_ref = ctx.matrix.users.get(uid)?;
    let user = user_ref.read().await;
    Some((user.user.clone(), user.host.clone(), user.nick.clone()))
}

/// Handler for SAJOIN command.
///
/// `SAJOIN nick channel`
///
/// Forces a user to join a channel.
pub struct SajoinHandler;

#[async_trait]
impl Handler for SajoinHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(oper_nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // SAJOIN <nick> <channel>
        let target_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender.send(err_needmoreparams(server_name, &oper_nick, "SAJOIN")).await?;
                return Ok(());
            }
        };
        let channel_name = match msg.arg(1) {
            Some(c) if !c.is_empty() => c,
            _ => {
                ctx.sender.send(err_needmoreparams(server_name, &oper_nick, "SAJOIN")).await?;
                return Ok(());
            }
        };

        // Find target user
        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            ctx.sender.send(err_nosuchnick(server_name, &oper_nick, target_nick)).await?;
            return Ok(());
        };

        // Validate channel name
        if !channel_name.starts_with('#') && !channel_name.starts_with('&') {
            ctx.sender.send(err_nosuchchannel(server_name, &oper_nick, channel_name)).await?;
            return Ok(());
        }

        let channel_lower = irc_to_lower(channel_name);

        // Get or create channel
        let channel_ref = ctx
            .matrix
            .channels
            .entry(channel_lower.clone())
            .or_insert_with(|| {
                Arc::new(RwLock::new(crate::state::Channel::new(channel_name.to_string())))
            })
            .clone();

        // Get target user info for JOIN message
        let Some((target_user, target_host, target_realname)) =
            get_user_prefix(ctx, &target_uid).await
        else {
            return Ok(());
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

        // Build and broadcast JOIN message
        let join_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(target_realname, target_user, target_host)),
            command: Command::JOIN(channel_name.to_string(), None, None),
        };
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
                format!("SAJOIN: {target_nick} has been forced to join {channel_name}"),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for SAPART command.
///
/// `SAPART nick channel [reason]`
///
/// Forces a user to leave a channel.
pub struct SapartHandler;

#[async_trait]
impl Handler for SapartHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(oper_nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // SAPART <nick> <channel>
        let target_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender.send(err_needmoreparams(server_name, &oper_nick, "SAPART")).await?;
                return Ok(());
            }
        };
        let channel_name = match msg.arg(1) {
            Some(c) if !c.is_empty() => c,
            _ => {
                ctx.sender.send(err_needmoreparams(server_name, &oper_nick, "SAPART")).await?;
                return Ok(());
            }
        };

        // Find target user
        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            ctx.sender.send(err_nosuchnick(server_name, &oper_nick, target_nick)).await?;
            return Ok(());
        };

        let channel_lower = irc_to_lower(channel_name);

        // Check if channel exists
        let Some(channel_ref) = ctx.matrix.channels.get(&channel_lower) else {
            ctx.sender.send(err_nosuchchannel(server_name, &oper_nick, channel_name)).await?;
            return Ok(());
        };

        // Get target user info for PART message
        let Some((target_user, target_host, target_realname)) =
            get_user_prefix(ctx, &target_uid).await
        else {
            return Ok(());
        };

        // Build and broadcast PART message (before removing member)
        let part_msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(target_realname, target_user, target_host)),
            command: Command::PART(channel_name.to_string(), None),
        };
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
            "SAPART: Forced user to leave channel"
        );

        // Confirm to operator
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                oper_nick,
                format!("SAPART: {target_nick} has been forced to leave {channel_name}"),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for SANICK command.
///
/// `SANICK oldnick newnick`
///
/// Forces a user to change their nickname.
pub struct SanickHandler;

#[async_trait]
impl Handler for SanickHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(oper_nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // SANICK <oldnick> <newnick>
        let old_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender.send(err_needmoreparams(server_name, &oper_nick, "SANICK")).await?;
                return Ok(());
            }
        };
        let new_nick = match msg.arg(1) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender.send(err_needmoreparams(server_name, &oper_nick, "SANICK")).await?;
                return Ok(());
            }
        };

        // Find target user
        let old_lower = irc_to_lower(old_nick);
        let Some(target_uid) = resolve_nick_to_uid(ctx, old_nick) else {
            ctx.sender.send(err_nosuchnick(server_name, &oper_nick, old_nick)).await?;
            return Ok(());
        };

        // Check if new nick is already in use
        let new_lower = irc_to_lower(new_nick);
        if ctx.matrix.nicks.contains_key(&new_lower) {
            let reply = server_reply(
                server_name,
                Response::ERR_NICKNAMEINUSE,
                vec![oper_nick.clone(), new_nick.to_string(), "Nickname is already in use".to_string()],
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
            prefix: Some(Prefix::Nickname(old_nick.to_string(), target_user, target_host)),
            command: Command::NICK(new_nick.to_string()),
        };

        // Update nick mapping
        ctx.matrix.nicks.remove(&old_lower);
        ctx.matrix.nicks.insert(new_lower, target_uid.clone());

        // Update user's nick
        if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
            let mut user = user_ref.write().await;
            user.nick = new_nick.to_string();
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
                format!("SANICK: {old_nick} has been forced to change nick to {new_nick}"),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}

/// Handler for SAMODE command.
///
/// `SAMODE channel modes [params]`
///
/// Sets channel modes as the server (bypassing op requirement).
pub struct SamodeHandler;

#[async_trait]
impl Handler for SamodeHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(oper_nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // SAMODE <channel> <modes> [params]
        let channel_name = match msg.arg(0) {
            Some(c) if !c.is_empty() => c,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &oper_nick, "SAMODE"))
                    .await?;
                return Ok(());
            }
        };
        let modes_str = match msg.arg(1) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &oper_nick, "SAMODE"))
                    .await?;
                return Ok(());
            }
        };

        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel = match ctx.matrix.channels.get(&channel_lower) {
            Some(c) => c.clone(),
            None => {
                ctx.sender
                    .send(err_nosuchchannel(server_name, &oper_nick, channel_name))
                    .await?;
                return Ok(());
            }
        };

        // Build the pieces array: [modes_str, ...remaining args] - avoid intermediate allocation
        let mut pieces: Vec<&str> = vec![modes_str];
        pieces.extend(msg.args().iter().skip(2).copied());

        let typed_modes = match Mode::as_channel_modes(&pieces) {
            Ok(modes) => modes,
            Err(e) => {
                // Invalid mode string - send notice to operator
                let notice = Message {
                    tags: None,
                    prefix: Some(Prefix::ServerName(server_name.clone())),
                    command: Command::NOTICE(
                        oper_nick.clone(),
                        format!("SAMODE error: {e}"),
                    ),
                };
                ctx.sender.send(notice).await?;
                return Ok(());
            }
        };

        // Apply modes to channel state
        let mut channel_guard = channel.write().await;
        let canonical_name = channel_guard.name.clone();

        let (applied, used_args) = apply_channel_modes_typed(ctx, &mut channel_guard, &typed_modes)?;

        if !applied.is_empty() {
            // Build the mode params for broadcast
            let mut mode_params = vec![canonical_name.clone(), applied.clone()];
            mode_params.extend(used_args);

            // Broadcast as server (not as user)
            let mode_msg = Message {
                tags: None,
                prefix: Some(Prefix::ServerName(server_name.clone())),
                command: Command::Raw("MODE".to_string(), mode_params),
            };

            // Broadcast to all channel members
            for uid in channel_guard.members.keys() {
                if let Some(sender) = ctx.matrix.senders.get(uid) {
                    let _ = sender.send(mode_msg.clone()).await;
                }
            }

            tracing::info!(
                oper = %oper_nick,
                channel = %canonical_name,
                modes = %applied,
                "SAMODE: Server mode change applied"
            );
        }

        // Confirm to operator
        let notice = Message {
            tags: None,
            prefix: Some(Prefix::ServerName(server_name.clone())),
            command: Command::NOTICE(
                oper_nick,
                format!("SAMODE: {canonical_name} {applied}"),
            ),
        };
        ctx.sender.send(notice).await?;

        Ok(())
    }
}
