//! ADMIN command handler.
//!
//! `ADMIN [target]`
//!
//! Returns administrative information about the server.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for ADMIN command.
pub struct AdminHandler;

#[async_trait]
impl PostRegHandler for AdminHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!
        let server_name = ctx.server_name();

        // RPL_ADMINME (256): <server> :Administrative info
        ctx.send_reply(
            Response::RPL_ADMINME,
            vec![
                nick.to_string(),
                server_name.to_string(),
                "Administrative info".to_string(),
            ],
        )
        .await?;

        // RPL_ADMINLOC1 (257): :<admin info> - organization/server description
        // Read from hot_config for hot-reload support
        let (admin_info1_opt, admin_info2_opt, admin_email_opt) = {
            let hot = ctx.matrix.hot_config.read();
            hot.admin_info.clone()
        };
        let admin_info1 =
            admin_info1_opt.unwrap_or_else(|| ctx.matrix.server_info.description.clone());
        ctx.send_reply(Response::RPL_ADMINLOC1, vec![nick.to_string(), admin_info1])
            .await?;

        // RPL_ADMINLOC2 (258): :<admin info> - location/network
        let admin_info2 = admin_info2_opt.unwrap_or_else(|| ctx.matrix.server_info.network.clone());
        ctx.send_reply(Response::RPL_ADMINLOC2, vec![nick.to_string(), admin_info2])
            .await?;

        // RPL_ADMINEMAIL (259): :<admin email>
        let admin_email = admin_email_opt.unwrap_or_else(|| format!("admin@{}", server_name));
        ctx.send_reply(
            Response::RPL_ADMINEMAIL,
            vec![nick.to_string(), admin_email],
        )
        .await?;

        Ok(())
    }
}
