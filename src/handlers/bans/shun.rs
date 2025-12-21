//! SHUN command handlers.
//!
//! SHUN silently ignores commands from matching users without disconnecting them.
//! This is less disruptive than traditional bans and useful for dealing with
//! automated abuse.

use crate::db::Shun;
use crate::handlers::{Context, HandlerResult, PostRegHandler, server_notice};
use crate::state::RegisteredState;
use crate::{require_arg_or_reply, require_oper_cap};
use async_trait::async_trait;
use slirc_proto::{MessageRef, Response};

/// Handler for SHUN command.
///
/// `SHUN [time] <mask> [reason]`
///
/// Silently ignores commands from matching users without disconnecting them.
pub struct ShunHandler;

#[async_trait]
impl PostRegHandler for ShunHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();
        let nick = ctx.nick();

        let Some(_cap) = require_oper_cap!(ctx, "SHUN", request_shun_cap) else {
            return Ok(());
        };
        let Some(mask) = require_arg_or_reply!(ctx, msg, 0, "SHUN") else {
            return Ok(());
        };
        let reason = msg.arg(1).unwrap_or("Shunned");

        // Store shun in database
        if let Err(e) = ctx.db.bans().add_shun(mask, Some(reason), nick, None).await {
            tracing::error!(error = %e, "Failed to add shun to database");
        } else {
            // Also add to in-memory cache for fast lookup
            let now = chrono::Utc::now().timestamp();
            ctx.matrix.security_manager.shuns.insert(
                mask.to_string(),
                Shun {
                    mask: mask.to_string(),
                    reason: Some(reason.to_string()),
                    set_by: nick.to_string(),
                    set_at: now,
                    expires_at: None,
                },
            );
        }

        tracing::info!(
            oper = %nick,
            mask = %mask,
            reason = %reason,
            "SHUN added"
        );

        // Send confirmation
        ctx.sender
            .send(server_notice(
                server_name,
                nick,
                format!("Shun added: {mask} ({reason})"),
            ))
            .await?;

        Ok(())
    }
}

/// Handler for UNSHUN command.
///
/// `UNSHUN <mask>`
///
/// Removes a shun.
pub struct UnshunHandler;

#[async_trait]
impl PostRegHandler for UnshunHandler {
    async fn handle(
        &self,
        ctx: &mut Context<'_, RegisteredState>,
        msg: &MessageRef<'_>,
    ) -> HandlerResult {
        let server_name = ctx.server_name();

        // Get nick and check capability
        let nick = ctx.nick();
        let authority = ctx.authority();
        let Some(_cap) = authority.request_shun_cap(ctx.uid).await else {
            let reply = Response::err_noprivileges(nick).with_prefix(ctx.server_prefix());
            ctx.send_error("UNSHUN", "ERR_NOPRIVILEGES", reply).await?;
            return Ok(());
        };

        // UNSHUN <mask>
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                let reply =
                    Response::err_needmoreparams(nick, "UNSHUN").with_prefix(ctx.server_prefix());
                ctx.send_error("UNSHUN", "ERR_NEEDMOREPARAMS", reply)
                    .await?;
                return Ok(());
            }
        };

        // Remove from database
        if let Err(e) = ctx.db.bans().remove_shun(mask).await {
            tracing::error!(error = %e, "Failed to remove shun from database");
        }

        // Remove from in-memory cache
        let removed = ctx.matrix.security_manager.shuns.remove(mask).is_some();

        if removed {
            tracing::info!(
                oper = %nick,
                mask = %mask,
                "SHUN removed"
            );

            // Send confirmation
            ctx.sender
                .send(server_notice(
                    server_name,
                    nick,
                    format!("Shun removed: {mask}"),
                ))
                .await?;
        } else {
            ctx.sender
                .send(server_notice(
                    server_name,
                    nick,
                    format!("Shun not found: {mask}"),
                ))
                .await?;
        }

        Ok(())
    }
}
