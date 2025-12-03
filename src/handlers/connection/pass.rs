//! PASS command handler for connection registration.

use super::super::{Context, Handler, HandlerResult, server_reply};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};
use tracing::debug;

/// Handler for PASS command.
///
/// `PASS password`
///
/// Sets the connection password before registration.
pub struct PassHandler;

#[async_trait]
impl Handler for PassHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // PASS must be sent before NICK/USER (RFC 2812 Section 3.1.1)
        if ctx.handshake.registered {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ALREADYREGISTERED,
                vec!["*".to_string(), "You may not reregister".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // PASS must come before NICK/USER
        if ctx.handshake.nick.is_some() || ctx.handshake.user.is_some() {
            let reply = server_reply(
                &ctx.matrix.server_info.name,
                Response::ERR_ALREADYREGISTERED,
                vec![
                    "*".to_string(),
                    "PASS must be sent before NICK/USER".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // PASS <password>
        let password = match msg.arg(0) {
            Some(p) if !p.is_empty() => p,
            _ => {
                let reply = server_reply(
                    &ctx.matrix.server_info.name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![
                        "*".to_string(),
                        "PASS".to_string(),
                        "Not enough parameters".to_string(),
                    ],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        ctx.handshake.pass_received = Some(password.to_string());
        debug!("PASS received");

        Ok(())
    }
}
