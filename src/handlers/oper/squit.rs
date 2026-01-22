//! SQUIT command handler - Terminates S2S links.
//!
//! Usage: `SQUIT <server> :<reason>`
//! Requires: IRC operator privileges
//!
//! Finds the target server by name or SID and broadcasts SQUIT to the network,
//! then removes the peer from SyncManager.

use super::super::{Context, HandlerResult, PostRegHandler, get_nick_or_star, server_notice};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{Command, Message, MessageRef, Response};
use std::sync::Arc;
use tracing::warn;

/// Handler for the SQUIT command.
///
/// Terminates an S2S link to a specified server.
pub struct SquitHandler;

#[async_trait]
impl PostRegHandler for SquitHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let nick = get_nick_or_star(ctx).await;

        // Require operator privileges via capability authority
        let authority = ctx.authority();
        if authority.request_squit_cap(ctx.uid).await.is_none() {
            let reply = Response::err_noprivileges(&nick).with_prefix(ctx.server_prefix());
            ctx.send_error("SQUIT", "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        }

        // Parse target server (required)
        let target = match msg.arg(0) {
            Some(t) => t,
            None => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        "SQUIT: Usage: SQUIT <server> :<reason>",
                    ))
                    .await?;
                return Ok(());
            }
        };

        let reason = msg.arg(1).unwrap_or("No reason given");

        // Find target server in topology by name or SID
        let target_sid = ctx
            .matrix
            .sync_manager
            .topology
            .servers
            .iter()
            .find(|e| e.value().name == target || e.key().as_str() == target)
            .map(|e| e.key().clone());

        let sid = match target_sid {
            Some(s) => s,
            None => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        format!("SQUIT: Server '{}' not found in network", target),
                    ))
                    .await?;
                return Ok(());
            }
        };

        // Broadcast SQUIT to network
        let squit_msg = Arc::new(Message::from(Command::SQUIT(
            sid.as_str().to_string(),
            reason.to_string(),
        )));
        ctx.matrix.sync_manager.broadcast(squit_msg, None).await;

        // Remove peer from local state
        ctx.matrix.sync_manager.remove_peer(&sid).await;

        ctx.sender
            .send(server_notice(
                server_name,
                &nick,
                format!("SQUIT: Disconnected {} ({})", target, reason),
            ))
            .await?;

        warn!(
            oper = %nick,
            target = %target,
            sid = %sid.as_str(),
            reason = %reason,
            "SQUIT command issued - S2S link terminated"
        );

        Ok(())
    }
}
