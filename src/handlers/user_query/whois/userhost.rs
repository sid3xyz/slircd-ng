//! USERHOST handler for resolving user@host pairs.

use crate::handlers::{
    Context, Handler, HandlerError, HandlerResult, err_notregistered, server_reply,
};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

/// Handler for USERHOST command.
///
/// `USERHOST nick [nick ...]`
///
/// Returns the user@host for up to 5 nicknames.
pub struct UserhostHandler;

#[async_trait]
impl Handler for UserhostHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // USERHOST <nick> [<nick> ...]
        let nicks = msg.args();

        if nicks.is_empty() {
            let reply = server_reply(
                server_name,
                Response::ERR_NEEDMOREPARAMS,
                vec![
                    nick.clone(),
                    "USERHOST".to_string(),
                    "Not enough parameters".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // Build response (up to 5 nicks)
        let mut replies = Vec::new();
        for target_nick in nicks.iter().take(5) {
            let target_lower = irc_to_lower(target_nick);
            let uid = ctx.matrix.nicks.get(&target_lower);
            let user_ref = uid.as_ref().and_then(|u| ctx.matrix.users.get(u.value()));
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
