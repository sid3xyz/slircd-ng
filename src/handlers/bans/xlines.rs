//! X-line ban command handlers.
//!
//! Server-wide ban commands (operator-only):
//! - KLINE/UNKLINE: Ban/unban by nick!user@host mask
//! - DLINE/UNDLINE: Ban/unban by IP address
//! - GLINE/UNGLINE: Global ban/unban by nick!user@host mask
//! - ZLINE/UNZLINE: Global IP ban/unban (skips DNS)
//! - RLINE/UNRLINE: Ban/unban by realname (GECOS)

use super::common::{BanType, disconnect_matching_ban};
use crate::handlers::{err_needmoreparams, require_oper, server_notice, Context, Handler, HandlerResult};
use async_trait::async_trait;
use slirc_proto::MessageRef;

// ============================================================================
// K-line (local user@host ban)
// ============================================================================

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

// ============================================================================
// D-line (local IP ban)
// ============================================================================

/// Handler for DLINE command.
///
/// `DLINE [time] ip :reason`
///
/// Bans an IP address from the server.
pub struct DlineHandler;

#[async_trait]
impl Handler for DlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // DLINE [time] <ip> <reason>
        let ip = match msg.arg(0) {
            Some(i) if !i.is_empty() => i,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "DLINE"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("No reason given");

        // Store D-line in database
        if let Err(e) = ctx.db.bans().add_dline(ip, Some(reason), &nick, None).await {
            tracing::error!(error = %e, "Failed to add D-line to database");
        }

        // Disconnect any matching users
        let disconnected = disconnect_matching_ban(ctx, BanType::Dline, ip, reason).await;

        tracing::info!(
            oper = %nick,
            ip = %ip,
            reason = %reason,
            disconnected = disconnected,
            "DLINE added"
        );

        // Send confirmation
        let text = if disconnected > 0 {
            format!("D-line added: {ip} ({reason}) - {disconnected} user(s) disconnected")
        } else {
            format!("D-line added: {ip} ({reason})")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}

/// Handler for UNDLINE command.
///
/// `UNDLINE ip`
///
/// Removes a D-line.
pub struct UndlineHandler;

#[async_trait]
impl Handler for UndlineHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // UNDLINE <ip>
        let ip = match msg.arg(0) {
            Some(i) if !i.is_empty() => i,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNDLINE"))
                    .await?;
                return Ok(());
            }
        };

        // Remove D-line from database
        let removed = match ctx.db.bans().remove_dline(ip).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove D-line from database");
                false
            }
        };

        if removed {
            tracing::info!(oper = %nick, ip = %ip, "UNDLINE removed");
        }

        // Send confirmation
        let text = if removed {
            format!("D-line removed: {ip}")
        } else {
            format!("No D-line found for: {ip}")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}

// ============================================================================
// G-line (global user@host ban)
// ============================================================================

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

// ============================================================================
// Z-line (global IP ban, skips DNS)
// ============================================================================

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

// ============================================================================
// R-line (realname/GECOS ban)
// ============================================================================

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
