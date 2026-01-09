//! PASS command handler for connection registration.

use super::super::{Context, HandlerResult, PreRegHandler};
use crate::state::UnregisteredState;
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tracing::debug;

/// Handler for PASS command.
///
/// `PASS password`
///
/// Sets the connection password before registration.
/// # RFC 2812 §3.1.1
///
/// Password message - Sets a connection password (must be sent before NICK/USER).
///
/// **Specification:** [RFC 2812 §3.1.1](https://datatracker.ietf.org/doc/html/rfc2812#section-3.1.1)
///
/// **Compliance:** 11/11 irctest pass
pub struct PassHandler;

#[async_trait]
impl PreRegHandler for PassHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, UnregisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // PASS must come before NICK/USER
        if ctx.state.nick.is_some() || ctx.state.user.is_some() {
            let reply = Response::err_alreadyregistred("*").with_prefix(ctx.server_prefix());
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // PASS <password>
        let Some(password) = crate::require_arg_or_reply!(ctx, msg, 0, "PASS") else {
            return Ok(());
        };

        ctx.state.pass_received = Some(password.to_string());
        debug!("PASS received");

        Ok(())
    }
}
