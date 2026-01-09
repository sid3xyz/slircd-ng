//! PING and PONG handlers.

use super::super::{Context, HandlerResult, UniversalHandler, with_label};
use crate::state::SessionState;
use async_trait::async_trait;
use slirc_proto::{Message, MessageRef};

pub struct PingHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for PingHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, msg: &MessageRef<'_>) -> HandlerResult {
        // PING <token>
        // Response: :<server> PONG <server> <token>
        // Per RFC 1459: PING requires at least one parameter
        let Some(token) = crate::require_arg_or_reply!(ctx, msg, 0, "PING") else {
            return Ok(());
        };
        let server_name = ctx.server_name();

        // PONG must have server prefix for clients to properly match responses
        let pong = Message::pong_with_token(server_name, token).with_prefix(ctx.server_prefix());
        // Attach label for labeled-response capability
        let pong = with_label(pong, ctx.label.as_deref());
        ctx.sender.send(pong).await?;

        Ok(())
    }
}

pub struct PongHandler;

#[async_trait]
impl<S: SessionState> UniversalHandler<S> for PongHandler {
    async fn handle(&self, ctx: &mut Context<'_, S>, _msg: &MessageRef<'_>) -> HandlerResult {
        // PONG normally produces no output, but with labeled-response we send ACK
        if let Some(label) = &ctx.label {
            let ack = super::super::labeled_ack(ctx.server_name(), label);
            ctx.sender.send(ack).await?;
        }

        // Just acknowledge PONG - resets idle timer (handled in connection loop)
        Ok(())
    }
}
