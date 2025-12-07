//! PING and PONG handlers.

use super::super::{Context, HandlerResult, UniversalHandler, err_needmoreparams, with_label};
use async_trait::async_trait;
use slirc_proto::{Message, MessageRef, prefix::Prefix};

/// Handler for PING command.
pub struct PingHandler;

#[async_trait]
impl UniversalHandler for PingHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        // PING <token>
        // Response: :<server> PONG <server> <token>
        // Per RFC 1459: PING requires at least one parameter
        let token = match msg.arg(0) {
            Some(t) if !t.is_empty() => t,
            _ => {
                // No token provided - return ERR_NEEDMOREPARAMS (461)
                let nick = ctx.state.nick.as_deref().unwrap_or("*");
                let reply = err_needmoreparams(&ctx.matrix.server_info.name, nick, "PING");
                let reply = with_label(reply, ctx.label.as_deref());
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };
        let server_name = &ctx.matrix.server_info.name;

        // PONG must have server prefix for clients to properly match responses
        let pong = Message::pong_with_token(server_name, token)
            .with_prefix(Prefix::ServerName(server_name.clone()));
        // Attach label for labeled-response capability
        let pong = with_label(pong, ctx.label.as_deref());
        ctx.sender.send(pong).await?;

        Ok(())
    }
}

/// Handler for PONG command.
pub struct PongHandler;

#[async_trait]
impl UniversalHandler for PongHandler {
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
