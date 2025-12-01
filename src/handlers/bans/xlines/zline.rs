//! Z-line (global IP ban, skips DNS) handlers.

use super::super::common::{BanType, disconnect_matching_ban};
use crate::handlers::{err_needmoreparams, require_oper, server_notice, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::MessageRef;

/// Handler for ZLINE command.
///
/// `ZLINE [time] ip :reason`
///
/// IP ban that skips DNS lookup (faster for abuse mitigation).
pub struct ZlineHandler;

#[async_trait]
impl Handler for ZlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // ZLINE [time] <ip> <reason>
        let ip = match msg.arg(0) {
            Some(i) if !i.is_empty() => i,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "ZLINE"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Store Z-line in database
        if let Err(e) = ctx.db.bans().add_zline(ip, Some(reason), &nick, None).await {
            tracing::error!(error = %e, "Failed to add Z-line to database");
        }

        // Disconnect any matching users
        let disconnected = disconnect_matching_ban(ctx, BanType::Zline, ip, reason).await;

        tracing::info!(
            oper = %nick,
            ip = %ip,
            reason = %reason,
            disconnected = disconnected,
            "ZLINE added"
        );

        // Send confirmation
        let text = if disconnected > 0 {
            format!("Z-line added: {ip} ({reason}) - {disconnected} user(s) disconnected")
        } else {
            format!("Z-line added: {ip} ({reason})")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}

/// Handler for UNZLINE command.
///
/// `UNZLINE ip`
///
/// Removes a Z-line.
pub struct UnzlineHandler;

#[async_trait]
impl Handler for UnzlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // UNZLINE <ip>
        let ip = match msg.arg(0) {
            Some(i) if !i.is_empty() => i,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNZLINE"))
                    .await?;
                return Ok(());
            }
        };

        // Remove Z-line from database
        let removed = match ctx.db.bans().remove_zline(ip).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove Z-line from database");
                false
            }
        };

        if removed {
            tracing::info!(oper = %nick, ip = %ip, "UNZLINE removed");
        }

        // Send confirmation
        let text = if removed {
            format!("Z-line removed: {ip}")
        } else {
            format!("No Z-line found for: {ip}")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}
