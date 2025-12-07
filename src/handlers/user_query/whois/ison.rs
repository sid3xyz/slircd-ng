//! ISON handler for online status queries.

use crate::handlers::{HandlerError, HandlerResult, PostRegHandler, server_reply};
use crate::handlers::core::traits::TypedContext;
use crate::state::Registered;
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
        ctx: &mut TypedContext<'_, Registered>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .state
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // ISON <nick> [<nick> ...]
        let nicks = msg.args();

        if nicks.is_empty() {
            let reply = server_reply(
                server_name,
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    nick.clone(),
                    "ISON".to_string(),
                    "Not enough parameters".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Find which nicks are online
        let mut online = Vec::new();
        for target_nick in nicks {
            let target_lower = irc_to_lower(target_nick);
            if ctx.matrix.nicks.contains_key(&target_lower) {
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
