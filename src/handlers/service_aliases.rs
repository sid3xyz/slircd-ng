//! Service command aliases: NS (NickServ), CS (ChanServ)
//!
//! Provides shortcut commands for interacting with IRC services.

use super::{Context, Handler, HandlerError, HandlerResult, err_notregistered};
use crate::services::route_service_message;
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for NS (NickServ alias) command.
///
/// `NS <command> [args]`
///
/// Shortcut for PRIVMSG NickServ.
pub struct NsHandler;

#[async_trait]
impl Handler for NsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Join all args into the command text
        let text = msg.args().join(" ");
        let cmd_text = if text.is_empty() { "HELP" } else { &text };

        // Route to NickServ via unified service router
        route_service_message(
            ctx.matrix, ctx.db, ctx.uid, nick, "NickServ", cmd_text, ctx.sender,
        )
        .await;

        Ok(())
    }
}

/// Handler for CS (ChanServ alias) command.
///
/// `CS <command> [args]`
///
/// Shortcut for PRIVMSG ChanServ.
pub struct CsHandler;

#[async_trait]
impl Handler for CsHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        if !ctx.handshake.registered {
            ctx.sender
                .send(err_notregistered(&ctx.matrix.server_info.name))
                .await?;
            return Ok(());
        }

        let nick = ctx
            .handshake
            .nick
            .as_ref()
            .ok_or(HandlerError::NickOrUserMissing)?;

        // Join all args into the command text
        let text = msg.args().join(" ");
        let cmd_text = if text.is_empty() { "HELP" } else { &text };

        // Route to ChanServ via unified service router
        route_service_message(
            ctx.matrix, ctx.db, ctx.uid, nick, "ChanServ", cmd_text, ctx.sender,
        )
        .await;

        Ok(())
    }
}
