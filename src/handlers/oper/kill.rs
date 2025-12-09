use super::super::{Context,
    HandlerResult, PostRegHandler,
    get_nick_or_star, resolve_nick_to_uid, user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Prefix, Response};

/// Handler for KILL command.
///
/// `KILL nickname :reason`
///
/// Disconnects a user from the network. Requires operator privileges.
/// Uses capability-based authorization (Innovation 4).
pub struct KillHandler;

#[async_trait]
impl PostRegHandler for KillHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let target_nick = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                let reply = Response::err_needmoreparams(&nick, "KILL")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("KILL", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Get killer's identity
        let Some((killer_nick, killer_user, killer_host)) =
            user_mask_from_state(ctx, ctx.uid).await
        else {
            return Ok(());
        };

        // Request KILL capability from authority (Innovation 4)
        // This replaces the legacy `if !is_oper` check with capability-based auth
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        let _kill_cap = match authority.request_kill_cap(ctx.uid).await {
            Some(cap) => cap, // Authorization granted - cap proves it
            None => {
                // Not an operator - no capability granted
                let reply = Response::err_noprivileges(&killer_nick)
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("KILL", "ERR_NOPRIVILEGES");
                return Ok(());
            }
        };

        let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) else {
            let reply = Response::err_nosuchnick(&killer_nick, target_nick)
                .with_prefix(Prefix::ServerName(server_name.to_string()));
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("KILL", "ERR_NOSUCHNICK");
            return Ok(());
        };

        let quit_reason = format!("Killed by {killer_nick} ({reason})");

        if let Some(target_sender) = ctx.matrix.senders.get(&target_uid) {
            let error_msg = Message {
                tags: None,
                prefix: None,
                command: Command::ERROR(format!("Closing Link: {} ({})", target_nick, quit_reason)),
            };
            let _ = target_sender.send(error_msg).await;
        }

        ctx.matrix.disconnect_user(&target_uid, &quit_reason).await;

        tracing::info!(killer = %killer_nick, target = %target_nick, reason = %reason, "KILL command executed");

        let kill_msg = Message {
            tags: None,
            prefix: Some(Prefix::new(
                killer_nick.clone(),
                killer_user,
                killer_host,
            )),
            command: Command::KILL(target_nick.to_string(), quit_reason),
        };

        let _ = ctx.sender.send(kill_msg).await;

        Ok(())
    }
}
