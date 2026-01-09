//! SERVICE, SERVLIST, and SQUERY handlers.
//!
//! RFC 2812 Section 3.1.6 (SERVICE) and Section 3.5 (SERVLIST, SQUERY).
//!
//! SERVICE is for registering as a service (not client) - we reject this.
//! SERVLIST lists registered services - we return an empty list.
//! SQUERY routes messages to services like NickServ/ChanServ.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
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
impl PostRegHandler for ServiceHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // If already registered as a user, send ERR_ALREADYREGISTERED
        let nick = ctx.nick();
        ctx.send_reply(
            Response::ERR_ALREADYREGISTERED,
            vec![nick.to_string(), "You are already registered".to_string()],
        )
        .await?;
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
impl PostRegHandler for ServlistHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        _msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick();

        // RPL_SERVLISTEND (235) - no services to list
        // Format: <nick> <mask> <type> :End of service listing
        ctx.send_reply(
            Response::RPL_SERVLISTEND,
            vec![
                nick.to_string(),
                "*".to_string(),
                "*".to_string(),
                "End of service listing".to_string(),
            ],
        )
        .await?;

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
impl PostRegHandler for SqueryHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick();

        let Some(service_name) = crate::require_arg_or_reply!(ctx, msg, 0, "SQUERY") else {
            return Ok(());
        };

        let text = match msg.arg(1) {
            Some(t) if !t.is_empty() => t,
            _ => {
                ctx.send_reply(
                    Response::ERR_NOTEXTTOSEND,
                    vec![nick.to_string(), "No text to send".to_string()],
                )
                .await?;
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
            ctx.send_reply(
                Response::ERR_NOSUCHSERVICE,
                vec![
                    nick.to_string(),
                    service_name.to_string(),
                    "No such service".to_string(),
                ],
            )
            .await?;
        }

        Ok(())
    }
}
