use super::super::{Context,
    HandlerResult, PostRegHandler,
    get_nick_or_star, resolve_nick_to_uid, server_notice,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

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
        let server_name = &ctx.matrix.server_info.name;
        let oper_nick = get_nick_or_star(ctx).await;

        // Request oper capability from authority
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        if authority.request_chgident_cap(ctx.uid).await.is_none() {
            let reply = Response::err_noprivileges(&oper_nick)
                .with_prefix(Prefix::ServerName(server_name.to_string()));
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("CHGIDENT", "ERR_NOPRIVILEGES");
            return Ok(());
        }

        let target_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                let reply = Response::err_needmoreparams(&oper_nick, "CHGIDENT")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHGIDENT", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        let new_ident = match msg.arg(1) {
            Some(u) if !u.is_empty() => u,
            _ => {
                let reply = Response::err_needmoreparams(&oper_nick, "CHGIDENT")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHGIDENT", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        // Validate ident length/chars? For now, just accept it.

        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            let reply = Response::err_nosuchnick(&oper_nick, target_nick)
                .with_prefix(Prefix::ServerName(server_name.to_string()));
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("CHGIDENT", "ERR_NOSUCHNICK");
            return Ok(());
        };

        let (old_nick, old_user, old_host, channels) = {
            let Some(user_ref) = ctx.matrix.users.get(&target_uid) else {
                let reply = Response::err_nosuchnick(&oper_nick, target_nick)
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHGIDENT", "ERR_NOSUCHNICK");
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
                .broadcast_to_channel_with_cap(
                    channel_name,
                    chghost_msg.clone(),
                    None,
                    Some("chghost"),
                    None,
                )
                .await;
        }

        if let Some(target_sender) = ctx.matrix.senders.get(&target_uid)
            && let Some(user_ref) = ctx.matrix.users.get(&target_uid)
        {
            let user = user_ref.read().await;
            if user.caps.contains("chghost") {
                let _ = target_sender.send(chghost_msg).await;
            }
        }

        ctx.sender
            .send(server_notice(
                server_name,
                &oper_nick,
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
