//! Admin command handlers (SA* commands).
//!
//! Server admin commands (operator-only):
//! - SAJOIN: Force a user to join a channel
//! - SAPART: Force a user to leave a channel
//! - SAMODE: Set channel modes as server
//! - SANICK: Force a user to change nick

use super::{
    Context, HandlerResult, PostRegHandler, TargetUser, err_needmoreparams, err_noprivileges,
    err_nosuchchannel, err_nosuchnick, force_join_channel, force_part_channel,
    format_modes_for_log, resolve_nick_to_uid, server_notice,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use crate::state::MemberModes;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Mode, Prefix, Response, irc_to_lower};

/// Get user prefix info (user, host, nick) for message construction.
async fn get_user_prefix<S>(ctx: &Context<'_, S>, uid: &str) -> Option<(String, String, String)> {
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
impl PostRegHandler for SajoinHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // Get nick and check admin capability
        let oper_nick = ctx.nick();
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let Some(_cap) = authority.request_admin_cap(ctx.uid).await else {
            ctx.sender
                .send(err_noprivileges(server_name, oper_nick))
                .await?;
            return Ok(());
        };

        // SAJOIN <nick> <channel>
        let target_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SAJOIN"))
                    .await?;
                return Ok(());
            }
        };
        let channel_name = match msg.arg(1) {
            Some(c) if !c.is_empty() => c,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SAJOIN"))
                    .await?;
                return Ok(());
            }
        };

        // Find target user
        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            ctx.sender
                .send(err_nosuchnick(server_name, oper_nick, target_nick))
                .await?;
            return Ok(());
        };

        // Validate channel name
        if !channel_name.starts_with('#') && !channel_name.starts_with('&') {
            ctx.sender
                .send(err_nosuchchannel(server_name, oper_nick, channel_name))
                .await?;
            return Ok(());
        }

        // Get target user info for JOIN message
        let Some((target_user, target_host, target_realname)) =
            get_user_prefix(ctx, &target_uid).await
        else {
            return Ok(());
        };

        // Get sender for the target user to send topic/names
        let target_sender = ctx.matrix.senders.get(&target_uid).map(|r| r.clone());

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
        let server_name = &ctx.matrix.server_info.name;

        // Get nick and check admin capability
        let oper_nick = ctx.nick();
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let Some(_cap) = authority.request_admin_cap(ctx.uid).await else {
            ctx.sender
                .send(err_noprivileges(server_name, oper_nick))
                .await?;
            return Ok(());
        };

        // SAPART <nick> <channel>
        let target_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SAPART"))
                    .await?;
                return Ok(());
            }
        };
        let channel_name = match msg.arg(1) {
            Some(c) if !c.is_empty() => c,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SAPART"))
                    .await?;
                return Ok(());
            }
        };

        // Find target user
        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            ctx.sender
                .send(err_nosuchnick(server_name, oper_nick, target_nick))
                .await?;
            return Ok(());
        };

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
            ctx.sender
                .send(err_nosuchchannel(server_name, oper_nick, channel_name))
                .await?;
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
        let server_name = &ctx.matrix.server_info.name;

        // Get nick and check admin capability
        let oper_nick = ctx.nick();
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let Some(_cap) = authority.request_admin_cap(ctx.uid).await else {
            ctx.sender
                .send(err_noprivileges(server_name, oper_nick))
                .await?;
            return Ok(());
        };

        // SANICK <oldnick> <newnick>
        let old_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SANICK"))
                    .await?;
                return Ok(());
            }
        };
        let new_nick = match msg.arg(1) {
            Some(n) if !n.is_empty() => n,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SANICK"))
                    .await?;
                return Ok(());
            }
        };

        // Find target user
        let old_lower = irc_to_lower(old_nick);
        let Some(target_uid) = resolve_nick_to_uid(ctx, old_nick) else {
            ctx.sender
                .send(err_nosuchnick(server_name, oper_nick, old_nick))
                .await?;
            return Ok(());
        };

        // Check if new nick is already in use
        let new_lower = irc_to_lower(new_nick);
        if ctx.matrix.nicks.contains_key(&new_lower) {
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
                old_nick.to_string(),
                target_user,
                target_host,
            )),
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
                ctx.matrix
                    .broadcast_to_channel(channel_name, nick_msg.clone(), None)
                    .await;
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
        let server_name = &ctx.matrix.server_info.name;

        // Get nick and check admin capability
        let oper_nick = ctx.nick();
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let Some(_cap) = authority.request_admin_cap(ctx.uid).await else {
            ctx.sender
                .send(err_noprivileges(server_name, oper_nick))
                .await?;
            return Ok(());
        };

        // SAMODE <channel> <modes> [params]
        let channel_name = match msg.arg(0) {
            Some(c) if !c.is_empty() => c,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SAMODE"))
                    .await?;
                return Ok(());
            }
        };
        let modes_str = match msg.arg(1) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, oper_nick, "SAMODE"))
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
                    .send(err_nosuchchannel(server_name, oper_nick, channel_name))
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
        let mut target_uids = std::collections::HashMap::new();
        for mode in &typed_modes {
            match mode.mode() {
                slirc_proto::mode::ChannelMode::Oper | slirc_proto::mode::ChannelMode::Voice => {
                    if let Some(nick) = mode.arg() {
                        let nick_lower = irc_to_lower(nick);
                        if let Some(uid) = ctx.matrix.nicks.get(&nick_lower) {
                            target_uids.insert(nick.to_string(), uid.value().clone());
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
                sender_uid: ctx.uid.to_string(),
                sender_prefix: slirc_proto::Prefix::ServerName(server_name.clone()),
                modes: typed_modes,
                target_uids,
                force: true,
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
