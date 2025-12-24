//! CHGIDENT command handler for operator ident changes.
//!
//! Allows operators to change a user's ident (username field).

use super::super::{
    notify_extended_monitor_watchers, Context, HandlerResult, PostRegHandler, resolve_nick_to_uid,
    server_notice,
};
use crate::state::RegisteredState;
use crate::{require_arg_or_reply, require_oper_cap};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};
use std::sync::Arc;

/// Handler for CHGIDENT command. Uses capability-based authorization.
///
/// `CHGIDENT <nick> <new_ident>`
///
/// Changes a user's displayed username (ident). Requires operator privileges.
/// Clients with the `chghost` capability receive a CHGHOST message.
pub struct ChgIdentHandler;

#[async_trait]
impl PostRegHandler for ChgIdentHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        // Request oper capability from authority
        let Some(_cap) = require_oper_cap!(ctx, "CHGIDENT", request_chgident_cap) else {
            return Ok(());
        };
        let Some(target_nick) = require_arg_or_reply!(ctx, msg, 0, "CHGIDENT") else {
            return Ok(());
        };
        let Some(new_ident) = require_arg_or_reply!(ctx, msg, 1, "CHGIDENT") else {
            return Ok(());
        };

        // Validate ident length/chars? For now, just accept it.

        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            let reply =
                Response::err_nosuchnick(oper_nick, target_nick).with_prefix(ctx.server_prefix());
            ctx.send_error("CHGIDENT", "ERR_NOSUCHNICK", reply).await?;
            return Ok(());
        };

        let (old_nick, old_user, old_host, channels) = {
            let Some(user_ref) = ctx.matrix.user_manager.users.get(&target_uid) else {
                let reply = Response::err_nosuchnick(oper_nick, target_nick)
                    .with_prefix(ctx.server_prefix());
                ctx.send_error("CHGIDENT", "ERR_NOSUCHNICK", reply).await?;
                return Ok(());
            };

            let mut user = user_ref.write().await;
            let old_nick = user.nick.clone();
            let old_user = user.user.clone();
            let old_host = user.visible_host.clone();
            let channels: Vec<String> = user.channels.iter().cloned().collect();

            user.user = new_ident.to_string();

            (old_nick, old_user, old_host, channels)
        };

        // Broadcast CHGHOST message (since CHGIDENT isn't a standard message for clients)
        // CHGHOST <user> <host>
        let chghost_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(
                old_nick.clone(),
                old_user.clone(),
                old_host.clone(),
            )),
            command: Command::CHGHOST(new_ident.to_string(), old_host.clone()),
        };

        for channel_name in &channels {
            ctx.matrix
                .channel_manager
                .broadcast_to_channel_with_cap(
                    channel_name,
                    chghost_msg.clone(),
                    None,
                    Some("chghost"),
                    None,
                )
                .await;
        }

        if let Some(target_sender) = ctx.matrix.user_manager.senders.get(&target_uid)
            && let Some(user_ref) = ctx.matrix.user_manager.users.get(&target_uid)
        {
            let user = user_ref.read().await;
            if user.caps.contains("chghost") {
                let _ = target_sender.send(Arc::new(chghost_msg.clone())).await;
            }
        }

        // Notify extended-monitor watchers (IRCv3 extended-monitor)
        notify_extended_monitor_watchers(ctx.matrix, &old_nick, chghost_msg, "chghost").await;

        ctx.sender
            .send(server_notice(
                server_name,
                oper_nick,
                format!(
                    "Changed ident of {} from {} to {}",
                    old_nick, old_user, new_ident
                ),
            ))
            .await?;

        tracing::info!(
            oper = %oper_nick,
            target = %old_nick,
            old_user = %old_user,
            new_user = %new_ident,
            "CHGIDENT command executed"
        );

        Ok(())
    }
}
