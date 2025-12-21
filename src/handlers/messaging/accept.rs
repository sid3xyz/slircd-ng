use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response, irc_to_lower};

/// Handler for ACCEPT command (Caller ID).
pub struct AcceptHandler;

#[async_trait]
impl PostRegHandler for AcceptHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let args = msg.arg(0).unwrap_or("");

        // Get user lock
        // We know the user exists because this is a PostRegHandler
        let user_arc = ctx.matrix.user_manager.users.get(ctx.uid).unwrap().clone();

        if args.is_empty() || args == "*" {
            // List entries
            let user = user_arc.read().await;
            for nick in &user.accept_list {
                let _ = ctx
                    .sender
                    .send(Response::rpl_acceptlist(&ctx.state.nick, nick))
                    .await;
            }
            let _ = ctx
                .sender
                .send(Response::rpl_endofaccept(&ctx.state.nick))
                .await;
            return Ok(());
        }

        let mut user = user_arc.write().await;

        for arg in args.split(',') {
            if arg.is_empty() {
                continue;
            }

            let (nick, remove) = if let Some(stripped) = arg.strip_prefix('-') {
                (stripped, true)
            } else {
                (arg, false)
            };

            let nick_lower = irc_to_lower(nick);

            if remove {
                if !user.accept_list.remove(&nick_lower) {
                    let _ = ctx
                        .sender
                        .send(Response::err_accept_not(&ctx.state.nick, nick))
                        .await;
                }
            } else if user.accept_list.contains(&nick_lower) {
                let _ = ctx
                    .sender
                    .send(Response::err_accept_exist(&ctx.state.nick, nick))
                    .await;
            } else if user.accept_list.len() >= 100 {
                // Limit
                let _ = ctx
                    .sender
                    .send(Response::err_accept_full(&ctx.state.nick))
                    .await;
            } else {
                user.accept_list.insert(nick_lower);
            }
        }

        Ok(())
    }
}
