//! GLOBOPS command handler for global operator messages.
//!
//! Sends a message to all operators with +g (globops) mode set.

use super::super::{Context, HandlerResult, PostRegHandler};
use crate::state::RegisteredState;
use crate::{require_arg_or_reply, send_noprivileges};
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for GLOBOPS command. Uses capability-based authorization.
///
/// `GLOBOPS :message`
///
/// Sends a message to all operators (specifically those subscribed to 'g' snomask).
pub struct GlobOpsHandler;

#[async_trait]
impl PostRegHandler for GlobOpsHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let Some(globops_text) = require_arg_or_reply!(ctx, msg, 0, "GLOBOPS") else {
            return Ok(());
        };
        let sender_nick = ctx.nick().to_string();

        // Request GlobalNotice capability from authority
        let authority = ctx.authority();
        if authority.request_globops_cap(ctx.uid).await.is_none() {
            send_noprivileges!(ctx, "GLOBOPS");
            return Ok(());
        }

        // Send via snomask 'g' (globops). Also deliver to 'o' (oper) subscribers
        // to ensure default oper notice subscriptions receive it.
        let text = format!("From {}: {}", sender_nick, globops_text);
        ctx.matrix.user_manager.send_snomask('g', &text).await;
        // Also deliver to opers regardless of snomask subscriptions
        ctx.matrix.user_manager.send_notice_to_opers(&text).await;

        Ok(())
    }
}
