//! USERIP command handler.
//!
//! `USERIP nickname [nickname...]`
//!
//! Returns the IP addresses of the specified nicknames.
//! This is an oper-only command.

use crate::handlers::{Context, HandlerResult, PostRegHandler, get_oper_info};
use crate::state::RegisteredState;
use crate::state::dashmap_ext::DashMapExt;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for USERIP command.
pub struct UseripHandler;

#[async_trait]
impl PostRegHandler for UseripHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = &ctx.state.nick;

        // Check for oper privileges
        let Some((_, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            let reply = Response::err_noprivileges(nick).with_prefix(ctx.server_prefix());
            ctx.send_error("USERIP", "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        }

        // Need at least one nickname
        let Some(_) = crate::require_arg_or_reply!(ctx, msg, 0, "USERIP") else {
            return Ok(());
        };

        // Collect all target nicknames from arguments
        let mut results = Vec::with_capacity(16);

        for i in 0..16 {
            // Limit to 16 nicknames
            let Some(target_nick) = msg.arg(i) else {
                break;
            };

            // Look up the user by nick
            let lower_nick = slirc_proto::irc_to_lower(target_nick);
            if let Some(uid) = ctx.matrix.user_manager.get_first_uid(&lower_nick) {
                let user_arc = ctx.matrix.user_manager.users.get_cloned(&uid);
                if let Some(user_arc) = user_arc {
                    let user = user_arc.read().await;
                    // Format: nick[*]=+user@host
                    // * indicates oper, + indicates away (or - if away)
                    let oper_flag = if user.modes.oper { "*" } else { "" };
                    let away_flag = if user.away.is_some() { "-" } else { "+" };
                    results.push(format!(
                        "{}{}={}{}@{}",
                        user.nick, oper_flag, away_flag, user.user, user.host
                    ));
                }
            }
        }

        // RPL_USERIP (340): <reply> [<reply> ...]
        ctx.send_reply(Response::RPL_USERIP, vec![nick.clone(), results.join(" ")])
            .await?;

        Ok(())
    }
}
