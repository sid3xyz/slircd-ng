use super::super::{Context,
    HandlerResult, PostRegHandler, get_nick_or_star,
    user_mask_from_state,
};
use crate::state::RegisteredState;
use crate::caps::CapabilityAuthority;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response};

/// Handler for GLOBOPS command. Uses capability-based authorization.
///
/// `GLOBOPS :message`
///
/// Sends a message to all operators (specifically those subscribed to 'g' snomask).
pub struct GlobOpsHandler;

#[async_trait]
impl PostRegHandler for GlobOpsHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let globops_text = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let nick = get_nick_or_star(ctx).await;
                let reply = Response::err_needmoreparams(&nick, "GLOBOPS")
                    .with_prefix(Prefix::ServerName(server_name.to_string()));
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("GLOBOPS", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        // Get sender's identity
        let Some((sender_nick, _, _)) =
            user_mask_from_state(ctx, ctx.uid).await
        else {
            return Ok(());
        };

        // Request GlobalNotice capability from authority (reusing this for now)
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        if authority.request_globops_cap(ctx.uid).await.is_none() {
            let reply = Response::err_noprivileges(&sender_nick)
                .with_prefix(Prefix::ServerName(server_name.to_string()));
            ctx.sender.send(reply).await?;
            crate::metrics::record_command_error("GLOBOPS", "ERR_NOPRIVILEGES");
            return Ok(());
        }

        // Send via snomask 'g'
        // Format: "From <nick>: <message>"
        ctx.matrix.send_snomask('g', &format!("From {}: {}", sender_nick, globops_text)).await;

        Ok(())
    }
}
