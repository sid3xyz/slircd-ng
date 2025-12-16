use super::super::{Context,
    HandlerResult, PostRegHandler,
    resolve_nick_to_uid, server_notice,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

/// Handler for CHGHOST command. Uses capability-based authorization (Innovation 4).
///
/// `CHGHOST <nick> <new_user> <new_host>`
///
/// Changes a user's displayed username and hostname. Requires operator privileges.
/// Clients with the `chghost` capability receive a CHGHOST message instead of
/// seeing the user quit and rejoin.
pub struct ChghostHandler;

#[async_trait]
impl PostRegHandler for ChghostHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        // Request oper capability from authority (Innovation 4)
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        if authority.request_chghost_cap(ctx.uid).await.is_none() {
            let reply = Response::err_noprivileges(oper_nick)
                .with_prefix(Prefix::ServerName(server_name.to_string()));
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("CHGHOST", "ERR_NOPRIVILEGES");
            return Ok(());
        }

        let target_nick = match msg.arg(0) {
            Some(n) if !n.is_empty() => n,
            _ => {
                let reply = Response::err_needmoreparams(oper_nick, "CHGHOST")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHGHOST", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        let new_user = match msg.arg(1) {
            Some(u) if !u.is_empty() => u,
            _ => {
                let reply = Response::err_needmoreparams(oper_nick, "CHGHOST")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHGHOST", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        let new_host = match msg.arg(2) {
            Some(h) if !h.is_empty() => h,
            _ => {
                let reply = Response::err_needmoreparams(oper_nick, "CHGHOST")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHGHOST", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            let reply = Response::err_nosuchnick(oper_nick, target_nick)
                .with_prefix(Prefix::ServerName(server_name.to_string()));
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("CHGHOST", "ERR_NOSUCHNICK");
            return Ok(());
        };

        let (old_nick, old_user, old_host, channels) = {
            let Some(user_ref) = ctx.matrix.users.get(&target_uid) else {
                let reply = Response::err_nosuchnick(oper_nick, target_nick)
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("CHGHOST", "ERR_NOSUCHNICK");
                return Ok(());
            };

            let mut user = user_ref.write().await;
            let old_nick = user.nick.clone();
            let old_user = user.user.clone();
            let old_host = user.visible_host.clone();
            let channels: Vec<String> = user.channels.iter().cloned().collect();

            user.user = new_user.to_string();
            user.visible_host = new_host.to_string();

            (old_nick, old_user, old_host, channels)
        };

        let chghost_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(
                old_nick.clone(),
                old_user.clone(),
                old_host.clone(),
            )),
            command: Command::CHGHOST(new_user.to_string(), new_host.to_string()),
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
                oper_nick,
                format!(
                    "Changed host of {} from {}@{} to {}@{}",
                    old_nick, old_user, old_host, new_user, new_host
                ),
            ))
            .await?;

        tracing::info!(
            oper = %oper_nick,
            target = %old_nick,
            old_user = %old_user,
            old_host = %old_host,
            new_user = %new_user,
            new_host = %new_host,
            "CHGHOST command executed"
        );

        Ok(())
    }
}
