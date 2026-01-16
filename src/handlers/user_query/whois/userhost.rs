//! USERHOST handler for resolving user@host pairs.

use crate::handlers::{Context, HandlerResult, PostRegHandler, server_reply};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

/// Handler for USERHOST command.
///
/// `USERHOST nick [nick ...]`
///
/// Returns the user@host for up to 5 nicknames.
pub struct UserhostHandler;

#[async_trait]
impl PostRegHandler for UserhostHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = &ctx.state.nick;

        // USERHOST <nick> [<nick> ...]
        let Some(_) = crate::require_arg_or_reply!(ctx, msg, 0, "USERHOST") else {
            return Ok(());
        };
        let nicks = msg.args();

        // Build response (up to 5 nicks)
        let mut replies = Vec::with_capacity(5);
        for target_nick in nicks.iter().take(5) {
            let target_lower = irc_to_lower(target_nick);
            let uid = ctx.matrix.user_manager.get_first_uid(&target_lower);
            let user_ref = uid
                .as_ref()
                .and_then(|u| ctx.matrix.user_manager.users.get(u));
            if let Some(user_ref) = user_ref {
                let user = user_ref.read().await;
                // Format: nick[*]=+/-hostname
                // * if oper, - if away, + if available (RFC 2812)
                let oper_flag = if user.modes.oper { "*" } else { "" };
                let away_flag = if user.away.is_some() { "-" } else { "+" };
                replies.push(format!(
                    "{}{}={}{}@{}",
                    user.nick, oper_flag, away_flag, user.user, user.visible_host
                ));
            }
        }

        // RPL_USERHOST (302)
        let reply = server_reply(
            server_name,
            Response::RPL_USERHOST,
            vec![nick.clone(), replies.join(" ")],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
