//! R-line (realname/GECOS ban) handlers.

use super::super::common::{BanType, disconnect_matching_ban};
use crate::handlers::{err_needmoreparams, require_oper, server_notice, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for RLINE command.
///
/// `RLINE [time] pattern :reason`
///
/// Ban by realname (GECOS field) using wildcard pattern.
pub struct RlineHandler;

#[async_trait]
impl Handler for RlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // RLINE [time] <pattern> <reason>
        let pattern = match msg.arg(0) {
            Some(p) if !p.is_empty() => p,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "RLINE"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Store R-line in database
        if let Err(e) = ctx
            .db
            .bans()
            .add_rline(pattern, Some(reason), &nick, None)
            .await
        {
            tracing::error!(error = %e, "Failed to add R-line to database");
        }

        // Disconnect any matching users
        let disconnected = disconnect_matching_ban(ctx, BanType::Rline, pattern, reason).await;

        tracing::info!(
            oper = %nick,
            pattern = %pattern,
            reason = %reason,
            disconnected = disconnected,
            "RLINE added"
        );

        // Send confirmation
        let text = if disconnected > 0 {
            format!("R-line added: {pattern} ({reason}) - {disconnected} user(s) disconnected")
        } else {
            format!("R-line added: {pattern} ({reason})")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}

/// Handler for UNRLINE command.
///
/// `UNRLINE pattern`
///
/// Removes an R-line.
pub struct UnrlineHandler;

#[async_trait]
impl Handler for UnrlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // UNRLINE <pattern>
        let pattern = match msg.arg(0) {
            Some(p) if !p.is_empty() => p,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNRLINE"))
                    .await?;
                return Ok(());
            }
        };

        // Remove R-line from database
        let removed = match ctx.db.bans().remove_rline(pattern).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove R-line from database");
                false
            }
        };

        if removed {
            tracing::info!(oper = %nick, pattern = %pattern, "UNRLINE removed");
        }

        // Send confirmation
        let text = if removed {
            format!("R-line removed: {pattern}")
        } else {
            format!("No R-line found for: {pattern}")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}
