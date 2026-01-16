//! Admin command handlers (SA* commands).
//!
//! Server admin commands (operator-only):
//! - SAJOIN: Force a user to join a channel
//! - SAPART: Force a user to leave a channel
//! - SAMODE: Set channel modes as server
//! - SANICK: Force a user to change nick

use super::{
    Context, HandlerResult, PostRegHandler, TargetUser, force_join_channel, force_part_channel,
    format_modes_for_log, resolve_nick_or_nosuchnick, server_notice,
};
use crate::state::MemberModes;
use crate::state::RegisteredState;
use crate::{require_admin_cap, require_arg_or_reply};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Mode, Prefix, Response, irc_to_lower};
use std::sync::Arc;

/// Get user prefix info (user, host, nick) for message construction.
async fn get_user_prefix<S>(ctx: &Context<'_, S>, uid: &str) -> Option<(String, String, String)> {
    let user_ref = ctx.matrix.user_manager.users.get(uid)?;
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
impl PostRegHandler for SajoinHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let Some(_cap) = require_admin_cap!(ctx, "SAJOIN") else {
            return Ok(());
        };
        let Some(target_nick) = require_arg_or_reply!(ctx, msg, 0, "SAJOIN") else {
            return Ok(());
        };
        let Some(channel_name) = require_arg_or_reply!(ctx, msg, 1, "SAJOIN") else {
            return Ok(());
        };

        // Find target user
        let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "SAJOIN", target_nick).await? else {
            return Ok(());
        };

        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        // Validate channel name
        if !channel_name.starts_with('#') && !channel_name.starts_with('&') {
            let reply = Response::err_nosuchchannel(oper_nick, channel_name)
                .with_prefix(ctx.server_prefix());
            ctx.send_error("SAJOIN", "ERR_NOSUCHCHANNEL", reply).await?;
            return Ok(());
        }

        // Get target user info for JOIN message
        let Some((target_user, target_host, target_realname)) =
            get_user_prefix(ctx, &target_uid).await
        else {
            return Ok(());
        };

        // Get sender for the target user to send topic/names
        let target_sender = ctx
            .matrix
            .user_manager
            .senders
            .get(&target_uid)
            .map(|r| r.clone());

        // Use shared force_join_channel helper
        let target = TargetUser {
            uid: &target_uid,
            nick: &target_realname,
            user: &target_user,
            host: &target_host,
        };
        force_join_channel(
            ctx,
            &target,
            channel_name,
            MemberModes::default(),
            target_sender.as_ref(),
        )
        .await?;

        tracing::info!(
            oper = %oper_nick,
            target = %target_nick,
            channel = %channel_name,
            "SAJOIN: Forced user to join channel"
        );

        // Confirm to operator
        ctx.sender
            .send(server_notice(
                server_name,
                oper_nick,
                format!("SAJOIN: {target_nick} has been forced to join {channel_name}"),
            ))
            .await?;

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
impl PostRegHandler for SapartHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let Some(_cap) = require_admin_cap!(ctx, "SAPART") else {
            return Ok(());
        };
        let Some(target_nick) = require_arg_or_reply!(ctx, msg, 0, "SAPART") else {
            return Ok(());
        };
        let Some(channel_name) = require_arg_or_reply!(ctx, msg, 1, "SAPART") else {
            return Ok(());
        };

        // Find target user
        let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "SAPART", target_nick).await? else {
            return Ok(());
        };

        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        let channel_lower = irc_to_lower(channel_name);

        // Get target user info for PART message
        let Some((target_user, target_host, target_realname)) =
            get_user_prefix(ctx, &target_uid).await
        else {
            return Ok(());
        };

        // Use shared force_part_channel helper
        let target = TargetUser {
            uid: &target_uid,
            nick: &target_realname,
            user: &target_user,
            host: &target_host,
        };
        let was_in_channel = force_part_channel(ctx, &target, &channel_lower, None).await?;

        if !was_in_channel {
            let reply = Response::err_nosuchchannel(oper_nick, channel_name)
                .with_prefix(ctx.server_prefix());
            ctx.send_error("SAPART", "ERR_NOSUCHCHANNEL", reply).await?;
            return Ok(());
        }

        tracing::info!(
            oper = %oper_nick,
            target = %target_nick,
            channel = %channel_name,
            "SAPART: Forced user to leave channel"
        );

        // Confirm to operator
        ctx.sender
            .send(server_notice(
                server_name,
                oper_nick,
                format!("SAPART: {target_nick} has been forced to leave {channel_name}"),
            ))
            .await?;

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
impl PostRegHandler for SanickHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let Some(_cap) = require_admin_cap!(ctx, "SANICK") else {
            return Ok(());
        };
        let Some(old_nick) = require_arg_or_reply!(ctx, msg, 0, "SANICK") else {
            return Ok(());
        };
        let Some(new_nick) = require_arg_or_reply!(ctx, msg, 1, "SANICK") else {
            return Ok(());
        };

        // Find target user
        let old_lower = irc_to_lower(old_nick);
        let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "SANICK", old_nick).await? else {
            return Ok(());
        };

        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        // Check if new nick is already in use
        let new_lower = irc_to_lower(new_nick);
        if ctx.matrix.user_manager.nicks.contains_key(&new_lower) {
            ctx.send_reply(
                Response::ERR_NICKNAMEINUSE,
                vec![
                    oper_nick.to_string(),
                    new_nick.to_string(),
                    "Nickname is already in use".to_string(),
                ],
            )
            .await?;
            return Ok(());
        }

        // Get target user info for NICK message
        let (target_user, target_host) = {
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.value().clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                (user.user.clone(), user.host.clone())
            } else {
                return Ok(());
            }
        };

        // Build NICK message
        let nick_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(old_nick.to_string(), target_user, target_host)),
            command: Command::NICK(new_nick.to_string()),
        };

        // Update nick mapping
        ctx.matrix.user_manager.nicks.remove(&old_lower);
        ctx.matrix
            .user_manager
            .nicks
            .insert(new_lower, vec![target_uid.clone()]);

        // Update user's nick
        let user_arc = ctx
            .matrix
            .user_manager
            .users
            .get(&target_uid)
            .map(|u| u.value().clone());
        if let Some(user_arc) = user_arc {
            let mut user = user_arc.write().await;
            user.nick = new_nick.to_string();
        }

        // Broadcast NICK change to all channels the user is in
        let target_channels = {
            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.value().clone());
            if let Some(user_arc) = user_arc {
                let user = user_arc.read().await;
                user.channels.iter().cloned().collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        };
        for channel_name in &target_channels {
            ctx.matrix
                .channel_manager
                .broadcast_to_channel(channel_name, nick_msg.clone(), None)
                .await;
        }

        // Also send to the target user
        let sender = ctx
            .matrix
            .user_manager
            .senders
            .get(&target_uid)
            .map(|s| s.value().clone());
        if let Some(sender) = sender {
            let _ = sender.send(Arc::new(nick_msg)).await;
        }

        tracing::info!(
            oper = %oper_nick,
            old_nick = %old_nick,
            new_nick = %new_nick,
            "SANICK: Forced nick change"
        );

        // Confirm to operator
        ctx.sender
            .send(server_notice(
                server_name,
                oper_nick,
                format!("SANICK: {old_nick} has been forced to change nick to {new_nick}"),
            ))
            .await?;

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
impl PostRegHandler for SamodeHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        let Some(_cap) = require_admin_cap!(ctx, "SAMODE") else {
            return Ok(());
        };
        let Some(channel_name) = require_arg_or_reply!(ctx, msg, 0, "SAMODE") else {
            return Ok(());
        };
        let Some(modes_str) = require_arg_or_reply!(ctx, msg, 1, "SAMODE") else {
            return Ok(());
        };

        let channel_lower = irc_to_lower(channel_name);

        // Get channel
        let channel = match ctx.matrix.channel_manager.channels.get(&channel_lower) {
            Some(c) => c.value().clone(),
            None => {
                let reply = Response::err_nosuchchannel(oper_nick, channel_name)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("SAMODE", "ERR_NOSUCHCHANNEL", reply).await?;
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
                ctx.sender
                    .send(server_notice(
                        server_name,
                        oper_nick,
                        format!("SAMODE error: {e}"),
                    ))
                    .await?;
                return Ok(());
            }
        };

        // Resolve target UIDs for user modes
        let mut target_uids = std::collections::HashMap::with_capacity(typed_modes.len());
        for mode in &typed_modes {
            match mode.mode() {
                slirc_proto::mode::ChannelMode::Oper | slirc_proto::mode::ChannelMode::Voice => {
                    if let Some(nick) = mode.arg() {
                        let nick_lower = irc_to_lower(nick);
                        if let Some(uid) = ctx.matrix.user_manager.get_first_uid(&nick_lower) {
                            target_uids.insert(nick.to_string(), uid);
                        }
                    }
                }
                _ => {}
            }
        }

        // Apply modes to channel state
        let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
        if (channel
            .send(crate::state::actor::ChannelEvent::ApplyModes {
                params: crate::state::actor::ModeParams {
                    sender_uid: ctx.uid.to_string(),
                    sender_prefix: ctx.server_prefix(),
                    modes: typed_modes,
                    target_uids,
                    force: true,
                },
                reply_tx,
            })
            .await)
            .is_err()
        {
            return Ok(()); // Channel died
        }

        let applied_modes = match reply_rx.await {
            Ok(Ok(m)) => m,
            Ok(Err(e)) => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        oper_nick,
                        format!("SAMODE error: {e}"),
                    ))
                    .await?;
                return Ok(());
            }
            Err(_) => return Ok(()),
        };

        if !applied_modes.is_empty() {
            // Format modes for logging
            let modes_str = format_modes_for_log(&applied_modes);

            tracing::info!(
                oper = %oper_nick,
                channel = %channel_name,
                modes = %modes_str,
                "SAMODE: Server mode change applied"
            );

            // Confirm to operator
            ctx.sender
                .send(server_notice(
                    server_name,
                    oper_nick,
                    format!("SAMODE: {channel_name} {modes_str}"),
                ))
                .await?;
        } else {
            // No modes applied - still confirm to operator
            ctx.sender
                .send(server_notice(
                    server_name,
                    oper_nick,
                    format!("SAMODE: {channel_name} (no modes applied)"),
                ))
                .await?;
        }

        Ok(())
    }
}
