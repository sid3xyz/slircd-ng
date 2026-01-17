//! INFO command handler.
//!
//! `INFO [target]`
//!
//! Returns information describing the server.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Server version string.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Handler for INFO command.
pub struct InfoHandler;

#[async_trait]
impl PostRegHandler for InfoHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!
        let server_name = ctx.server_name();

        // If a target is specified, check if it matches this server
        if let Some(target) = msg.arg(0) {
            // Accept if target matches our server name exactly, or as nick
            let target_lower = target.to_lowercase();
            let server_lower = server_name.to_lowercase();
            let nick_lower = nick.to_lowercase();

            // Check if target matches server name or nick
            // Also accept wildcards that would match our server (simple * check)
            let is_match = target_lower == server_lower
                || target_lower == nick_lower
                || target == "*"
                || (target.ends_with('*')
                    && server_lower.starts_with(&target_lower[..target_lower.len() - 1]));

            if !is_match {
                // ERR_NOSUCHSERVER (402)
                ctx.send_reply(
                    Response::ERR_NOSUCHSERVER,
                    vec![
                        nick.to_string(),
                        target.to_string(),
                        "No such server".to_string(),
                    ],
                )
                .await?;
                return Ok(());
            }
        }

        let info_lines = [
            format!("slircd-ng v{} - High-performance IRC daemon", VERSION),
            "https://github.com/sid3xyz/slircd-ng".to_string(),
            "".to_string(),
            "Built with Rust and Tokio async runtime".to_string(),
            "Zero-copy message parsing via slirc-proto".to_string(),
            "DashMap concurrent state management".to_string(),
            "".to_string(),
            format!("Server: {}", ctx.server_name()),
            format!("Network: {}", ctx.matrix.server_info.network),
        ];

        // RPL_INFO (371): :<string>
        for line in &info_lines {
            ctx.send_reply(Response::RPL_INFO, vec![nick.to_string(), line.clone()])
                .await?;
        }

        // RPL_ENDOFINFO (374): :End of INFO list
        ctx.send_reply(
            Response::RPL_ENDOFINFO,
            vec![nick.to_string(), "End of INFO list".to_string()],
        )
        .await?;

        Ok(())
    }
}
