//! ISON handler for online status queries.

use crate::handlers::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

/// Handler for ISON command.
///
/// `ISON nick [nick ...]`
///
/// Returns which of the given nicknames are online.
pub struct IsonHandler;

#[async_trait]
impl PostRegHandler for IsonHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick;

        // ISON <nick> [<nick> ...]
        let Some(_) = crate::require_arg_or_reply!(ctx, msg, 0, "ISON") else {
            return Ok(());
        };
        let nicks = msg.args();

        // Find which nicks are online
        let mut online = Vec::with_capacity(nicks.len());
        for target_nick in nicks {
            let target_lower = irc_to_lower(target_nick);
            if ctx.matrix.user_manager.nicks.contains_key(&target_lower) {
                // Return the nick as the user typed it (case preserved)
                online.push((*target_nick).to_string());
            }
        }

        // RPL_ISON (303)
        let reply = server_reply(
            server_name,
            Response::RPL_ISON,
            vec![nick.clone(), online.join(" ")],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
