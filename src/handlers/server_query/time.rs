//! TIME command handler.
//!
//! `TIME [target]`
//!
//! Returns the local time on the server.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for TIME command.
pub struct TimeHandler;

#[async_trait]
impl PostRegHandler for TimeHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Compile-time guarantee: nick is always present for Registered connections
        let nick = ctx.nick(); // Returns &str, not Option!
        let server_name = ctx.server_name();

        // RPL_TIME (391): <server> :<string showing server's local time>
        let now = chrono::Local::now();
        let time_string = now.format("%A %B %d %Y -- %H:%M:%S %z").to_string();

        ctx.send_reply(
            Response::RPL_TIME,
            vec![nick.to_string(), server_name.to_string(), time_string],
        )
        .await?;

        Ok(())
    }
}
