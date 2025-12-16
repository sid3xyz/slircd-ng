use super::super::{Context,
    HandlerResult, PostRegHandler,
    resolve_nick_to_uid, server_reply,
};
use crate::require_oper_cap;
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for TRACE command. Uses capability-based authorization (Innovation 4).
///
/// `TRACE [target]`
///
/// Traces route to a server or user. For single-server implementation,
/// shows local connection information.
pub struct TraceHandler;

#[async_trait]
impl PostRegHandler for TraceHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let oper_nick = ctx.nick();

        // Request oper capability from authority (Innovation 4)
        // TRACE requires oper privileges (uses KillCap as a general oper check)
        let Some(_cap) = require_oper_cap!(ctx, "TRACE", request_kill_cap) else { return Ok(()); };

        let target = msg.arg(0);

        if let Some(target_nick) = target {
            if let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) {
                let user_arc = ctx.matrix.users.get(&target_uid).map(|u| u.value().clone());
                if let Some(user_arc) = user_arc {
                    let (target_nick, is_oper) = {
                        let user = user_arc.read().await;
                        (user.nick.clone(), user.modes.oper)
                    };

                    let numeric = if is_oper {
                        Response::RPL_TRACEOPERATOR
                    } else {
                        Response::RPL_TRACEUSER
                    };

                    let class = if is_oper { "Oper" } else { "User" };

                    let reply = server_reply(
                        server_name,
                        numeric,
                        vec![oper_nick.to_string(), class.to_string(), target_nick],
                    );
                    ctx.sender.send(reply).await?;
                }
            } else {
                let reply = Response::err_nosuchnick(oper_nick, target_nick)
                    .with_prefix(ctx.server_prefix());
                ctx.sender.send(reply).await?;
                crate::metrics::record_command_error("TRACE", "ERR_NOSUCHNICK");
                return Ok(());
            }
        } else {
            let user_arcs = ctx
                .matrix
                .users
                .iter()
                .map(|entry| entry.value().clone())
                .collect::<Vec<_>>();

            for user_arc in user_arcs {
                let (nick, is_oper) = {
                    let user = user_arc.read().await;
                    (user.nick.clone(), user.modes.oper)
                };

                let numeric = if is_oper {
                    Response::RPL_TRACEOPERATOR
                } else {
                    Response::RPL_TRACEUSER
                };

                let class = if is_oper { "Oper" } else { "User" };

                let reply = server_reply(
                    server_name,
                    numeric,
                    vec![oper_nick.to_string(), class.to_string(), nick],
                );
                ctx.sender.send(reply).await?;
            }
        }

        let end_reply = server_reply(
            server_name,
            Response::RPL_TRACEEND,
            vec![
                oper_nick.to_string(),
                server_name.to_string(),
                "End of TRACE".to_string(),
            ],
        );
        ctx.sender.send(end_reply).await?;

        tracing::debug!(oper = %oper_nick, target = ?target, "TRACE command executed");

        Ok(())
    }
}
