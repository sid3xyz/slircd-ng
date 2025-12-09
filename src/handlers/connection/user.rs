//! USER command handler for connection registration.

use super::super::{Context, HandlerError, HandlerResult, PreRegHandler};
use super::welcome::send_welcome_burst;
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response};
use tracing::debug;

/// Handler for USER command.
pub struct UserHandler;

#[async_trait]
impl PreRegHandler for UserHandler {
    async fn handle(&self, ctx: &mut Context<'_, UnregisteredState>, msg: &MessageRef<'_>) -> HandlerResult {
        // USER cannot be resent after already set
        if ctx.state.user.is_some() {
            let nick = ctx.state.nick.as_deref().unwrap_or("*");
            let reply = Response::err_alreadyregistred(nick)
                .with_prefix(Prefix::ServerName(ctx.matrix.server_info.name.clone()));
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
