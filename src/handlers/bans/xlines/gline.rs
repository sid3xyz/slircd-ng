//! G-line (global user@host ban) handlers.

use super::super::common::{BanType, disconnect_matching_ban};
use crate::handlers::{err_needmoreparams, require_oper, server_notice, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for GLINE command.
///
/// `GLINE [time] user@host :reason`
///
/// Global ban by nick!user@host mask (network-wide).
pub struct GlineHandler;

#[async_trait]
impl Handler for GlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // GLINE [time] <user@host> <reason>
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "GLINE"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Store G-line in database
        if let Err(e) = ctx
            .db
            .bans()
            .add_gline(mask, Some(reason), &nick, None)
            .await
        {
            tracing::error!(error = %e, "Failed to add G-line to database");
        }

        // Update in-memory cache for immediate effect
        ctx.matrix.ban_cache.add_gline(
            mask.to_string(),
            reason.to_string(),
            None, // No expiration for now
        );

        // Disconnect any matching users
        let disconnected = disconnect_matching_ban(ctx, BanType::Gline, mask, reason).await;

        tracing::info!(
            oper = %nick,
            mask = %mask,
            reason = %reason,
            disconnected = disconnected,
            "GLINE added"
        );

        // Send confirmation
        let text = if disconnected > 0 {
            format!("G-line added: {mask} ({reason}) - {disconnected} user(s) disconnected")
        } else {
            format!("G-line added: {mask} ({reason})")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}

/// Handler for UNGLINE command.
///
/// `UNGLINE user@host`
///
/// Removes a G-line.
pub struct UnglineHandler;

#[async_trait]
impl Handler for UnglineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // UNGLINE <mask>
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNGLINE"))
                    .await?;
                return Ok(());
            }
        };

        // Remove G-line from database
        let removed = match ctx.db.bans().remove_gline(mask).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove G-line from database");
                false
            }
        };

        // Remove from in-memory cache
        ctx.matrix.ban_cache.remove_gline(mask);

        if removed {
            tracing::info!(oper = %nick, mask = %mask, "UNGLINE removed");
        }

        // Send confirmation
        let text = if removed {
            format!("G-line removed: {mask}")
        } else {
            format!("No G-line found for: {mask}")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}
