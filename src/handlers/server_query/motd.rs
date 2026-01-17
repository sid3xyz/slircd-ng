//! MOTD command handler.
//!
//! `MOTD [target]`
//!
//! Returns the "Message of the Day" for the server.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for MOTD command.
pub struct MotdHandler;

#[async_trait]
impl PostRegHandler for MotdHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let server_name = ctx.server_name();
        let nick = ctx.nick();

        // RPL_MOTDSTART (375): :- <server> Message of the day -
        ctx.send_reply(
            Response::RPL_MOTDSTART,
            vec![
                nick.to_string(),
                format!("- {} Message of the day -", server_name),
            ],
        )
        .await?;

        // RPL_MOTD (372): :- <text> - send each line from configured MOTD
        // Read from hot_config for hot-reload support, clone before await
        let motd_lines = ctx.matrix.hot_config.read().motd_lines.clone();
        for line in &motd_lines {
            ctx.send_reply(
                Response::RPL_MOTD,
                vec![nick.to_string(), format!("- {}", line)],
            )
            .await?;
        }

        // RPL_ENDOFMOTD (376): :End of MOTD command
        ctx.send_reply(
            Response::RPL_ENDOFMOTD,
            vec![nick.to_string(), "End of MOTD command".to_string()],
        )
        .await?;

        Ok(())
    }
}
