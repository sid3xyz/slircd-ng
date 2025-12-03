//! PING and PONG handlers.

use super::super::{Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::{Message, MessageRef};

/// Handler for PING command.
pub struct PingHandler;

#[async_trait]
impl Handler for PingHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // PING <server>
        let server = msg.arg(0).unwrap_or("");

        let pong = Message::pong(server);
        ctx.sender.send(pong).await?;

        Ok(())
    }
}

/// Handler for PONG command.
pub struct PongHandler;

#[async_trait]
impl Handler for PongHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        // PONG normally produces no output, but with labeled-response we send ACK
        if let Some(label) = &ctx.label {
            let ack = super::super::labeled_ack(&ctx.matrix.server_info.name, label);
            ctx.sender.send(ack).await?;
        }

        // Just acknowledge PONG - resets idle timer (handled in connection loop)
        Ok(())
    }
}

/// Handler for QUIT command.
pub struct QuitHandler;

#[async_trait]
impl Handler for QuitHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let quit_msg = msg.arg(0).map(|s| s.to_string());

        tracing::info!(
            uid = %ctx.uid,
            nick = ?ctx.handshake.nick,
            message = ?quit_msg,
            "Client quit"
        );

        // Signal quit by returning Quit error that connection loop will handle
        Err(super::super::HandlerError::Quit(quit_msg))
    }
}
