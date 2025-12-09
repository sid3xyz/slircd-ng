//! PASS command handler for connection registration.

use super::super::{Context, HandlerResult, PreRegHandler};
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Prefix, Response};
use tracing::debug;

/// Handler for PASS command.
///
/// `PASS password`
///
/// Sets the connection password before registration.
pub struct PassHandler;

#[async_trait]
impl PreRegHandler for PassHandler {
    async fn handle(&self, ctx: &mut Context<'_, UnregisteredState>, msg: &MessageRef<'_>) -> HandlerResult {
        // PASS must come before NICK/USER
        if ctx.state.nick.is_some() || ctx.state.user.is_some() {
            let reply = Response::err_alreadyregistred("*")
                .with_prefix(Prefix::ServerName(ctx.matrix.server_info.name.clone()));
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // PASS <password>
        let password = match msg.arg(0) {
            Some(p) if !p.is_empty() => p,
            _ => {
                let reply = Response::err_needmoreparams("*", "PASS")
                    .with_prefix(Prefix::ServerName(ctx.matrix.server_info.name.clone()));
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        ctx.state.pass_received = Some(password.to_string());
        debug!("PASS received");

        Ok(())
    }
}
