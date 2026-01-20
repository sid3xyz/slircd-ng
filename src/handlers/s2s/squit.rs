//! SQUIT command handler.
//!
//! `SQUIT <server> <comment>`
//!
//! Disconnects a server link.

use crate::handlers::{
    Context, HandlerResult, PostRegHandler, get_oper_info, server_notice, server_reply,
};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for SQUIT command.
pub struct SquitHandler;

#[async_trait]
impl PostRegHandler for SquitHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Check oper privileges
        let Some((_, is_oper)) = get_oper_info(ctx).await else {
            return Ok(());
        };

        if !is_oper {
            let reply =
                Response::err_noprivileges(&ctx.state.nick).with_prefix(ctx.server_prefix());
            ctx.send_error("SQUIT", "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        }

        let server_mask = msg
            .arg(0)
            .ok_or(crate::handlers::HandlerError::NeedMoreParams)?;
        let comment = msg.arg(1).unwrap_or("Operator requested disconnect");

        // 1. Resolve server name to SID using TopologyGraph
        // Since TopologyGraph doesn't index by name, we iterate.
        // For production scale, a name->SID map would be better, but O(N) is fine for N<1000 servers.
        let target_sid = ctx
            .matrix
            .sync_manager
            .topology
            .servers
            .iter()
            .find(|entry| entry.value().name.eq_ignore_ascii_case(server_mask))
            .map(|entry| entry.key().clone());

        let Some(sid) = target_sid else {
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_NOSUCHSERVER,
                vec![
                    ctx.state.nick.clone(),
                    server_mask.to_string(),
                    "No such server".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        };

        if sid == ctx.matrix.sync_manager.local_id {
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_NOSUCHSERVER,
                vec![
                    ctx.state.nick.clone(),
                    server_mask.to_string(),
                    "Cannot SQUIT local server".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // 2. Check if it's a direct peer
        if let Some(link) = ctx.matrix.sync_manager.get_peer_for_server(&sid) {
            // It's a direct link - send ERROR and disconnect
            let error_msg = slirc_proto::Command::ERROR(format!("Closing link: {}", comment));
            let _ = link
                .tx
                .send(std::sync::Arc::new(slirc_proto::Message::from(error_msg)))
                .await;

            // Allow a brief moment for the message to traverse the channel before forceful close?
            // In async land, the 'remove_peer' might close the channel immediately.
            // But 'tx.send' is async. The receiver loop processes it.
            // If we remove it from 'links' map, the receiver loop might error out or close.
            // The receiver loop holds a reference to 'links', but iterating it from 'links' map.
            // Actually, removing from links map stops heartbeats and new routings.
            // The connection task itself might still be running.
            // For now, simple removal trigger is sufficient as `remove_peer` is all we have exposed.

            ctx.matrix.sync_manager.remove_peer(&sid).await;

            let reply = server_notice(
                ctx.server_name(),
                &ctx.state.nick,
                format!("Closed link to {}", server_mask),
            );
            ctx.sender.send(reply).await?;
        } else {
            // It's a remote server - we can't directly disconnect it, but we could send a SQUIT message?
            // In strict TS6/P10, SQUIT is propagated.
            // For now, to be safe, we only allow SQUITing direct peers.
            let reply = server_reply(
                ctx.server_name(),
                Response::ERR_NOSUCHSERVER, // Or specific error "Not a direct link"
                vec![
                    ctx.state.nick.clone(),
                    server_mask.to_string(),
                    "Can only SQUIT direct peers".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        }

        Ok(())
    }
}
