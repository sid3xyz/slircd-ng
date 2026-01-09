//! TRACE command handler for connection debugging.
//!
//! Shows routing and connection information for debugging purposes.
//! Operator-only command per RFC 2812.

use super::super::{
    Context, HandlerResult, PostRegHandler, resolve_nick_or_nosuchnick, server_reply,
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
        // Request oper capability from authority (Innovation 4)
        // TRACE requires oper privileges (uses KillCap as a general oper check)
        let Some(_cap) = require_oper_cap!(ctx, "TRACE", request_kill_cap) else {
            return Ok(());
        };

        let target = msg.arg(0);

        if let Some(target_nick) = target {
            let Some(target_uid) = resolve_nick_or_nosuchnick(ctx, "TRACE", target_nick).await?
            else {
                return Ok(());
            };

            let server_name = ctx.server_name();
            let oper_nick = ctx.nick();

            let user_arc = ctx
                .matrix
                .user_manager
                .users
                .get(&target_uid)
                .map(|u| u.value().clone());
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
            let server_name = ctx.server_name();
            let oper_nick = ctx.nick();

            let user_arcs = ctx
                .matrix
                .user_manager
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
            ctx.server_name(),
            Response::RPL_TRACEEND,
            vec![
                ctx.nick().to_string(),
                ctx.server_name().to_string(),
                "End of TRACE".to_string(),
            ],
        );
        ctx.sender.send(end_reply).await?;

        tracing::debug!(oper = %ctx.nick(), target = ?target, "TRACE command executed");

        Ok(())
    }
}
