//! USER command handler for connection registration.

use super::super::{Context, HandlerError, HandlerResult, PreRegHandler, server_reply};
use super::welcome::send_welcome_burst;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tracing::debug;

/// Handler for USER command.
pub struct UserHandler;

#[async_trait]
impl PreRegHandler for UserHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if ctx.state.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ALREADYREGISTERED,
                vec![
                    ctx.state
                        .nick
                        .clone()
                        .unwrap_or_else(|| "*".to_string()),
                    "You may not reregister".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // USER <username> <mode> <unused> <realname>
        let username = msg.arg(0).ok_or(HandlerError::NeedMoreParams)?;
        // arg(1) is mode, arg(2) is unused
        let realname = msg.arg(3).unwrap_or("");

        if username.is_empty() || realname.is_empty() {
            return Err(HandlerError::NeedMoreParams);
        }

        ctx.state.user = Some(username.to_string());
        ctx.state.realname = Some(realname.to_string());

        debug!(user = %username, realname = %realname, uid = %ctx.uid, "User set");

        // Check if we can complete registration
        if ctx.state.can_register() {
            send_welcome_burst(ctx).await?;
        }

        Ok(())
    }
}
