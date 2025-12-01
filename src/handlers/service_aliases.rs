//! Service command aliases: NS (NickServ), CS (ChanServ)
//!
//! Provides shortcut commands for interacting with IRC services.

use super::{Context, Handler, HandlerError, HandlerResult, err_notregistered};
use crate::services::chanserv::route_chanserv_message;
use crate::services::nickserv::route_service_message;
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

        if text.is_empty() {
            // Show help
            route_service_message(
                ctx.matrix, ctx.db, ctx.uid, nick, "NickServ", "HELP", ctx.sender,
            )
            .await;
        } else {
            // Route to NickServ
            route_service_message(
                ctx.matrix, ctx.db, ctx.uid, nick, "NickServ", &text, ctx.sender,
            )
            .await;
        }

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

        if text.is_empty() {
            // Show help
            route_chanserv_message(
                ctx.matrix, ctx.db, ctx.uid, nick, "ChanServ", "HELP", ctx.sender,
            )
            .await;
        } else {
            // Route to ChanServ
            route_chanserv_message(
                ctx.matrix, ctx.db, ctx.uid, nick, "ChanServ", &text, ctx.sender,
            )
            .await;
        }

        Ok(())
    }
}
