//! Service command aliases: NS (NickServ), CS (ChanServ)
//!
//! Provides shortcut commands for interacting with IRC services.

use crate::handlers::{Context, HandlerResult, PostRegHandler};
use crate::services::route_service_message;
use crate::state::RegisteredState;
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for NS (NickServ alias) command.
///
/// `NS <command> [args]`
///
/// Shortcut for PRIVMSG NickServ.
pub struct NsHandler;

#[async_trait]
impl PostRegHandler for NsHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick();

        // Join all args into the command text
        let text = msg.args().join(" ");
        let cmd_text = if text.is_empty() { "HELP" } else { &text };

        // Route to NickServ via unified service router
        route_service_message(ctx.matrix, ctx.uid, nick, "NickServ", cmd_text, &ctx.sender).await;

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
impl PostRegHandler for CsHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        // Registration check removed - handled by registry typestate dispatch (Innovation 1)

        let nick = ctx.nick();

        // Join all args into the command text
        let text = msg.args().join(" ");
        let cmd_text = if text.is_empty() { "HELP" } else { &text };

        // Route to ChanServ via unified service router
        route_service_message(ctx.matrix, ctx.uid, nick, "ChanServ", cmd_text, &ctx.sender).await;

        Ok(())
    }
}
