//! SERVICE, SERVLIST, and SQUERY handlers.
//!
//! RFC 2812 Section 3.1.6 (SERVICE) and Section 3.5 (SERVLIST, SQUERY).
//!
//! SERVICE is for registering as a service (not client) - we reject this.
//! SERVLIST lists registered services - we return an empty list.
//! SQUERY routes messages to services like NickServ/ChanServ.

use super::super::{Context, Handler, HandlerResult, err_notregistered, server_reply};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for SERVICE command.
///
/// `SERVICE nickname reserved distribution type reserved info`
///
/// Used by services to register. Clients cannot use this.
/// Returns ERR_ALREADYREGISTERED if already registered, otherwise rejects.
pub struct ServiceHandler;

#[async_trait]
impl Handler for ServiceHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        // If already registered as a user, send ERR_ALREADYREGISTERED
        if ctx.handshake.registered {
            let nick = ctx.handshake.nick.as_deref().unwrap_or("*");
            let reply = server_reply(
                server_name,
                Response::ERR_ALREADYREGISTERED,
                vec![nick.to_string(), "You are already registered".to_string()],
            );
            ctx.sender.send(reply).await?;
            return Ok(());
        }

        // SERVICE registration is not supported for clients
        // Send ERR_NOPRIVILEGES (481) - most servers do this
        let reply = server_reply(
            server_name,
            Response::ERR_NOPRIVILEGES,
            vec![
                "*".to_string(),
                "SERVICE command is not available".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for SERVLIST command.
///
/// `SERVLIST [mask [type]]`
///
/// Lists services matching the mask. Since we don't have services registered
/// via SERVICE command, we return an empty list.
pub struct ServlistHandler;

#[async_trait]
impl Handler for ServlistHandler {
    async fn handle(&self, ctx: &mut Context<'_>, _msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_deref().unwrap_or("*");

        // RPL_SERVLISTEND (235) - no services to list
        // Format: <nick> <mask> <type> :End of service listing
        let reply = server_reply(
            server_name,
            Response::RPL_SERVLISTEND,
            vec![
                nick.to_string(),
                "*".to_string(),
                "*".to_string(),
                "End of service listing".to_string(),
            ],
        );
        ctx.sender.send(reply).await?;

        Ok(())
    }
}

/// Handler for SQUERY command.
///
/// `SQUERY servicename text`
///
/// Sends a message to a service. We route to NickServ/ChanServ.
pub struct SqueryHandler;

#[async_trait]
impl Handler for SqueryHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let server_name = &ctx.matrix.server_info.name;
        let nick = ctx.handshake.nick.as_deref().unwrap_or("*");

        let service_name = match msg.arg(0) {
            Some(s) if !s.is_empty() => s,
            _ => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NEEDMOREPARAMS,
                    vec![nick.to_string(), "SQUERY".to_string(), "Not enough parameters".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        let text = match msg.arg(1) {
            Some(t) if !t.is_empty() => t,
            _ => {
                let reply = server_reply(
                    server_name,
                    Response::ERR_NOTEXTTOSEND,
                    vec![nick.to_string(), "No text to send".to_string()],
                );
                ctx.sender.send(reply).await?;
                return Ok(());
            }
        };

        // Route to NickServ or ChanServ using unified service router
        let handled = crate::services::route_service_message(
            ctx.matrix,
            ctx.uid,
            nick,
            service_name,
            text,
            &ctx.sender,
        )
        .await;

        if !handled {
            // Unknown service
            let reply = server_reply(
                server_name,
                Response::ERR_NOSUCHSERVICE,
                vec![
                    nick.to_string(),
                    service_name.to_string(),
                    "No such service".to_string(),
                ],
            );
            ctx.sender.send(reply).await?;
        }

        Ok(())
    }
}
