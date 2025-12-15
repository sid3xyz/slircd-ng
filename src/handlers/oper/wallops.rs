use super::super::{Context,
    HandlerResult, PostRegHandler, get_nick_or_star,
    user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

/// Handler for WALLOPS command. Uses capability-based authorization (Innovation 4).
///
/// `WALLOPS :message`
///
/// Sends a message to all users with +w mode (operators).
/// # RFC 2812 §4.7
///
/// Wallops - Sends message to users with +w mode or operators.
///
/// **Specification:** [RFC 2812 §4.7](https://datatracker.ietf.org/doc/html/rfc2812#section-4.7)
///
/// **Compliance:** 2/2 irctest pass
pub struct WallopsHandler;

#[async_trait]
impl PostRegHandler for WallopsHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let wallops_text = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                let reply = Response::err_needmoreparams(&nick, "WALLOPS")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("WALLOPS", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        // Get sender's identity
        let Some((sender_nick, sender_user, sender_host)) =
            user_mask_from_state(ctx, ctx.uid).await
        else {
            return Ok(());
        };

        // Request GlobalNotice capability from authority (Innovation 4)
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let _wallops_cap = match authority.request_wallops_cap(ctx.uid).await {
            Some(cap) => cap,
            None => {
                let reply = Response::err_noprivileges(&sender_nick)
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("WALLOPS", "ERR_NOPRIVILEGES");
                return Ok(());
            }
        };

        let wallops_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(
                sender_nick.clone(),
                sender_user.clone(),
                sender_host.clone(),
            )),
            command: Command::WALLOPS(wallops_text.to_string()),
        };

        // Always echo WALLOPS back to sender (per Modern IRC spec: servers MAY do this)
        let _ = ctx.sender.send(wallops_msg.clone()).await;

        // Send to all users with +w (wallops) or +o (oper) modes, except the sender
        for user_entry in ctx.matrix.users.iter() {
            let user = user_entry.read().await;
            if user.uid != ctx.uid
                && (user.modes.wallops || user.modes.oper)
                && let Some(sender) = ctx.matrix.senders.get(&user.uid)
            {
                let _ = sender.send(wallops_msg.clone()).await;
            }
        }

        Ok(())
    }
}
