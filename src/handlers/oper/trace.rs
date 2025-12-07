use super::super::{
    Context, Handler, HandlerResult, err_noprivileges, err_nosuchnick, get_nick_or_star,
    resolve_nick_to_uid, server_reply,
};
use crate::caps::CapabilityAuthority;
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
impl Handler for TraceHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;
        let oper_nick = get_nick_or_star(ctx).await;

        // Request oper capability from authority (Innovation 4)
        // TRACE requires oper privileges (uses KillCap as a general oper check)
        let authority = CapabilityAuthority::new(ctx.matrix.clone());
        if authority.request_kill_cap(ctx.uid).await.is_none() {
            ctx.sender
                .send(err_noprivileges(server_name, &oper_nick))
                .await?;
            return Ok(());
        }

        let target = msg.arg(0);

        if let Some(target_nick) = target {
            if let Some(target_uid) = resolve_nick_to_uid(ctx, target_nick) {
                if let Some(user_ref) = ctx.matrix.users.get(&target_uid) {
                    let user = user_ref.read().await;

                    let numeric = if user.modes.oper {
                        Response::RPL_TRACEOPERATOR
                    } else {
                        Response::RPL_TRACEUSER
                    };

                    let class = if user.modes.oper { "Oper" } else { "User" };

                    let reply = server_reply(
                        server_name,
                        numeric,
                        vec![oper_nick.clone(), class.to_string(), user.nick.clone()],
                    );
                    ctx.sender.send(reply).await?;
                }
            } else {
                ctx.sender
                    .send(err_nosuchnick(server_name, &oper_nick, target_nick))
                    .await?;
                return Ok(());
            }
        } else {
            for user_entry in ctx.matrix.users.iter() {
                let user = user_entry.read().await;

                let numeric = if user.modes.oper {
                    Response::RPL_TRACEOPERATOR
                } else {
                    Response::RPL_TRACEUSER
                };

                let class = if user.modes.oper { "Oper" } else { "User" };

                let reply = server_reply(
                    server_name,
                    numeric,
                    vec![oper_nick.clone(), class.to_string(), user.nick.clone()],
                );
                ctx.sender.send(reply).await?;
            }
        }

        let end_reply = server_reply(
            server_name,
            Response::RPL_TRACEEND,
            vec![
                oper_nick.clone(),
                server_name.clone(),
                "End of TRACE".to_string(),
            ],
        );
        ctx.sender.send(end_reply).await?;

        tracing::debug!(oper = %oper_nick, target = ?target, "TRACE command executed");

        Ok(())
    }
}
