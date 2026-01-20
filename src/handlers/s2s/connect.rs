//! CONNECT command handler.
//!
//! `CONNECT <target_server> [port [remote_server]]`
//!
//! Instructs the server to attempt an outbound connection to another server.

use crate::handlers::{
    Context, HandlerResult, PostRegHandler, get_oper_info, server_notice, server_reply,
};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for CONNECT command.
pub struct ConnectHandler;

#[async_trait]
impl PostRegHandler for ConnectHandler {
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
            ctx.send_error("CONNECT", "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        }

        let target_server = msg
            .arg(0)
            .ok_or(crate::handlers::HandlerError::NeedMoreParams)?;
        // Optional params for ad-hoc connection: CONNECT <target> <port> [remote_server]
        let port_str = msg.arg(1);
        let _remote_server = msg.arg(2); // In standard IRC this might proxy the CONNECT, but we'll focus on direct for now

        // 1. Check if it's a configured link
        let link_config = ctx
            .matrix
            .sync_manager
            .configured_links
            .iter()
            .find(|l| l.name.eq_ignore_ascii_case(target_server))
            .cloned();

        // 2. If not configured, check if we have enough info for ad-hoc connection
        if link_config.is_none() {
            if let Some(_port) = port_str.and_then(|p| p.parse::<u16>().ok()) {
                // Construct ad-hoc config
                let reply = server_reply(
                    ctx.server_name(),
                    Response::ERR_NOSUCHSERVER,
                    vec![
                        ctx.state.nick.clone(),
                        target_server.to_string(),
                        "Ad-hoc connections not supported. Please add to config.".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            } else {
                let reply = server_reply(
                    ctx.server_name(),
                    Response::ERR_NOSUCHSERVER,
                    vec![
                        ctx.state.nick.clone(),
                        target_server.to_string(),
                        "Server not found in configuration".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        }

        let config = link_config.unwrap();

        ctx.matrix.sync_manager.connect_to_peer(
            ctx.matrix.clone(),
            ctx.registry.clone(),
            ctx.db.clone(),
            config,
        );

        let reply = server_notice(
            ctx.server_name(),
            &ctx.state.nick,
            format!("Attempting to connect to {}", target_server),
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}
