//! CHGIDENT command handler for operator ident changes.
//!
//! Allows operators to change a user's ident (username field).

use super::super::{
    Context, HandlerResult, PostRegHandler, notify_extended_monitor_watchers,
    resolve_nick_or_nosuchnick, server_notice,
};
use crate::state::RegisteredState;
use crate::state::dashmap_ext::DashMapExt;
use crate::{require_arg_or_reply, require_oper_cap};
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix};
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

        let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "CHGIDENT", target_nick).await?
        else {
            return Ok(());
        };

        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        let (old_nick, old_user, old_host, channels) = {
            let Some(user_ref) = ctx.matrix.user_manager.users.get(&target_uid) else {
                crate::handlers::send_no_such_nick(ctx, "CHGIDENT", target_nick).await?;
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

        let target_sender = ctx.matrix.user_manager.senders.get_cloned(&target_uid);
        let target_user_arc = ctx.matrix.user_manager.users.get_cloned(&target_uid);
        if let (Some(target_sender), Some(target_user_arc)) = (target_sender, target_user_arc) {
            let user = target_user_arc.read().await;
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
