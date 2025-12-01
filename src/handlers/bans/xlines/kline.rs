//! K-line (local user@host ban) handlers.

use super::super::common::{BanType, disconnect_matching_ban};
use crate::handlers::{err_needmoreparams, require_oper, server_notice, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for KLINE command.
///
/// `KLINE [time] user@host :reason`
///
/// Bans a user mask from the server.
pub struct KlineHandler;

#[async_trait]
impl Handler for KlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // KLINE [time] <user@host> <reason>
        // For now, assume first arg is mask, second is reason
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "KLINE"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Store K-line in database
        if let Err(e) = ctx
            .db
            .bans()
            .add_kline(mask, Some(reason), &nick, None)
            .await
        {
            tracing::error!(error = %e, "Failed to add K-line to database");
        }

        // Disconnect any matching users
        let disconnected = disconnect_matching_ban(ctx, BanType::Kline, mask, reason).await;

        tracing::info!(
            oper = %nick,
            mask = %mask,
            reason = %reason,
            disconnected = disconnected,
            "KLINE added"
        );

        // Send confirmation
        let text = if disconnected > 0 {
            format!("K-line added: {mask} ({reason}) - {disconnected} user(s) disconnected")
        } else {
            format!("K-line added: {mask} ({reason})")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}

/// Handler for UNKLINE command.
///
/// `UNKLINE user@host`
///
/// Removes a K-line.
pub struct UnklineHandler;

#[async_trait]
impl Handler for UnklineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // UNKLINE <mask>
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNKLINE"))
                    .await?;
                return Ok(());
            }
        };

        // Remove K-line from database
        let removed = match ctx.db.bans().remove_kline(mask).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove K-line from database");
                false
            }
        };

        if removed {
            tracing::info!(oper = %nick, mask = %mask, "UNKLINE removed");
        }

        // Send confirmation
        let text = if removed {
            format!("K-line removed: {mask}")
        } else {
            format!("No K-line found for: {mask}")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}
