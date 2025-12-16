use super::super::{Context,
    HandlerResult, PostRegHandler,
};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

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
        let sender_nick = ctx.nick();

        let globops_text = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let reply = Response::err_needmoreparams(sender_nick, "GLOBOPS")
                    .with_prefix(ctx.server_prefix());
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("GLOBOPS", "ERR_NEEDMOREPARAMS");
                return Ok(());
            }
        };

        // Request GlobalNotice capability from authority (reusing this for now)
        let authority = ctx.authority();
        if authority.request_globops_cap(ctx.uid).await.is_none() {
            let reply = Response::err_noprivileges(sender_nick)
                .with_prefix(ctx.server_prefix());
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
