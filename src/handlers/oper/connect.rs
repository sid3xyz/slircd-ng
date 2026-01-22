//! CONNECT command handler - Initiates S2S links.
//!
//! Usage: `CONNECT <server> [port]`
//! Requires: IRC operator privileges
//!
//! Looks up the target server in the configured `[[link]]` blocks and initiates
//! an outbound connection using `SyncManager::connect_to_peer()`.

use super::super::{
    Context, HandlerResult, PostRegHandler, get_nick_or_star, server_notice,
};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tracing::info;

/// Handler for the CONNECT command.
///
/// Initiates an outbound S2S connection to a configured server.
pub struct ConnectHandler;

#[async_trait]
impl PostRegHandler for ConnectHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let nick = get_nick_or_star(ctx).await;

        // Require operator privileges via capability authority
        let authority = ctx.authority();
        if authority.request_connect_cap(ctx.uid).await.is_none() {
            let reply = Response::err_noprivileges(&nick).with_prefix(ctx.server_prefix());
            ctx.send_error("CONNECT", "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        }

        // Parse target server name (required)
        let target = match msg.arg(0) {
            Some(t) => t,
            None => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        "CONNECT: Usage: CONNECT <server> [port]",
                    ))
                    .await?;
                return Ok(());
            }
        };

        // Optional port override (not commonly used, for forward compatibility)
        let _port_override: Option<u16> = msg.arg(1).and_then(|p| p.parse().ok());

        // Look up target in configured link blocks from MatrixConfig
        let link_block = ctx
            .matrix
            .config
            .links
            .iter()
            .find(|l| l.name == target);

        let link = match link_block {
            Some(l) => l.clone(),
            None => {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        format!("CONNECT: No link block found for '{}'", target),
                    ))
                    .await?;
                return Ok(());
            }
        };

        // Check if already linked to prevent duplicate connections
        if let Some(sid) = &link.sid {
            let sid = slirc_proto::sync::clock::ServerId::new(sid.clone());
            if ctx.matrix.sync_manager.links.contains_key(&sid) {
                ctx.sender
                    .send(server_notice(
                        server_name,
                        &nick,
                        format!(
                            "CONNECT: Already linked to {} (SID {})",
                            target,
                            sid.as_str()
                        ),
                    ))
                    .await?;
                return Ok(());
            }
        }

        // Initiate the connection via SyncManager
        ctx.matrix.sync_manager.connect_to_peer(
            ctx.matrix.clone(),
            ctx.registry.clone(),
            ctx.db.clone(),
            link.clone(),
        );

        ctx.sender
            .send(server_notice(
                server_name,
                &nick,
                format!(
                    "CONNECT: Initiating connection to {} ({}:{})",
                    link.name, link.hostname, link.port
                ),
            ))
            .await?;

        info!(
            oper = %nick,
            target = %link.name,
            hostname = %link.hostname,
            port = %link.port,
            "CONNECT command issued - initiating S2S link"
        );

        Ok(())
    }
}
