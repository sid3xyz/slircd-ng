//! Ban command handlers.
//!
//! Commands for server bans (operator-only):
//! - KLINE: Ban by nick!user@host mask
//! - DLINE: Ban by IP address
//! - UNKLINE: Remove a K-line
//! - UNDLINE: Remove a D-line

use super::{Context, Handler, HandlerResult, err_needmoreparams, require_oper, server_notice};
use async_trait::async_trait;
use slirc_proto::{MessageRef, wildcard_match};

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
        let disconnected = disconnect_matching_kline(ctx, mask, reason).await;

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

/// Disconnect all users matching a K-line mask.
async fn disconnect_matching_kline(ctx: &Context<'_>, mask: &str, reason: &str) -> usize {
    let mut disconnected = 0;
    let mut to_disconnect = Vec::new();

    // Find matching users
    for entry in ctx.matrix.users.iter() {
        let uid = entry.key().clone();
        let user = entry.value().read().await;
        let user_host = format!("{}@{}", user.user, user.host);

        if wildcard_match(mask, &user_host) {
            to_disconnect.push(uid);
        }
    }

    // Disconnect them
    for uid in to_disconnect {
        let quit_reason = format!("K-lined: {}", reason);
        ctx.matrix.disconnect_user(&uid, &quit_reason).await;
        disconnected += 1;
    }

    disconnected
}

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
        let disconnected = disconnect_matching_dline(ctx, ip, reason).await;

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

/// Disconnect all users matching a D-line (IP ban).
async fn disconnect_matching_dline(ctx: &Context<'_>, mask: &str, reason: &str) -> usize {
    let mut disconnected = 0;
    let mut to_disconnect = Vec::new();

    // Find matching users by IP (stored in host field for now)
    for entry in ctx.matrix.users.iter() {
        let uid = entry.key().clone();
        let user = entry.value().read().await;

        // Check if user's host/IP matches the D-line
        if wildcard_match(mask, &user.host) || cidr_match(mask, &user.host) {
            to_disconnect.push(uid);
        }
    }

    // Disconnect them
    for uid in to_disconnect {
        let quit_reason = format!("D-lined: {}", reason);
        ctx.matrix.disconnect_user(&uid, &quit_reason).await;
        disconnected += 1;
    }

    disconnected
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
        let disconnected = disconnect_matching_gline(ctx, mask, reason).await;

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

/// Disconnect all users matching a G-line mask.
async fn disconnect_matching_gline(ctx: &Context<'_>, mask: &str, reason: &str) -> usize {
    let mut disconnected = 0;
    let mut to_disconnect = Vec::new();

    // Find matching users
    for entry in ctx.matrix.users.iter() {
        let uid = entry.key().clone();
        let user = entry.value().read().await;
        let user_host = format!("{}@{}", user.user, user.host);

        if wildcard_match(mask, &user_host) {
            to_disconnect.push(uid);
        }
    }

    // Disconnect them
    for uid in to_disconnect {
        let quit_reason = format!("G-lined: {}", reason);
        ctx.matrix.disconnect_user(&uid, &quit_reason).await;
        disconnected += 1;
    }

    disconnected
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
        let disconnected = disconnect_matching_zline(ctx, ip, reason).await;

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

/// Disconnect all users matching a Z-line (IP ban).
async fn disconnect_matching_zline(ctx: &Context<'_>, mask: &str, reason: &str) -> usize {
    let mut disconnected = 0;
    let mut to_disconnect = Vec::new();

    // Find matching users by IP (stored in host field for now)
    for entry in ctx.matrix.users.iter() {
        let uid = entry.key().clone();
        let user = entry.value().read().await;

        // Check if user's host/IP matches the Z-line
        if wildcard_match(mask, &user.host) || cidr_match(mask, &user.host) {
            to_disconnect.push(uid);
        }
    }

    // Disconnect them
    for uid in to_disconnect {
        let quit_reason = format!("Z-lined: {}", reason);
        ctx.matrix.disconnect_user(&uid, &quit_reason).await;
        disconnected += 1;
    }

    disconnected
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
        let disconnected = disconnect_matching_rline(ctx, pattern, reason).await;

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

/// Disconnect all users matching an R-line (realname ban).
async fn disconnect_matching_rline(ctx: &Context<'_>, pattern: &str, reason: &str) -> usize {
    let mut disconnected = 0;
    let mut to_disconnect = Vec::new();

    // Find matching users by realname
    for entry in ctx.matrix.users.iter() {
        let uid = entry.key().clone();
        let user = entry.value().read().await;

        if wildcard_match(pattern, &user.realname) {
            to_disconnect.push(uid);
        }
    }

    // Disconnect them
    for uid in to_disconnect {
        let quit_reason = format!("R-lined: {}", reason);
        ctx.matrix.disconnect_user(&uid, &quit_reason).await;
        disconnected += 1;
    }

    disconnected
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

/// Handler for SHUN command.
///
/// `SHUN [time] <mask> [reason]`
///
/// Silently ignores commands from matching users without disconnecting them.
pub struct ShunHandler;

#[async_trait]
impl Handler for ShunHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // SHUN [time] <mask> [reason]
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "SHUN"))
                    .await?;
                return Ok(());
            }
        };
        let reason = msg.arg(1).unwrap_or("Shunned");

        // Store shun in database
        if let Err(e) = ctx
            .db
            .bans()
            .add_shun(mask, Some(reason), &nick, None)
            .await
        {
            tracing::error!(error = %e, "Failed to add shun to database");
        }

        tracing::info!(
            oper = %nick,
            mask = %mask,
            reason = %reason,
            "SHUN added"
        );

        // Send confirmation
        ctx.sender.send(server_notice(server_name, &nick, format!("Shun added: {mask} ({reason})"))).await?;

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
impl Handler for UnshunHandler {
    async fn handle(&self, ctx: &mut Context<'_>, msg: &MessageRef<'_>) -> HandlerResult {
        let server_name = &ctx.matrix.server_info.name;

        let Ok(nick) = require_oper(ctx).await else {
            return Ok(());
        };

        // UNSHUN <mask>
        let mask = match msg.arg(0) {
            Some(m) if !m.is_empty() => m,
            _ => {
                ctx.sender
                    .send(err_needmoreparams(server_name, &nick, "UNSHUN"))
                    .await?;
                return Ok(());
            }
        };

        // Remove shun from database
        let removed = match ctx.db.bans().remove_shun(mask).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove shun from database");
                false
            }
        };

        if removed {
            tracing::info!(oper = %nick, mask = %mask, "UNSHUN removed");
        }

        // Send confirmation
        let text = if removed {
            format!("Shun removed: {mask}")
        } else {
            format!("No shun found for: {mask}")
        };
        ctx.sender.send(server_notice(server_name, &nick, &text)).await?;

        Ok(())
    }
}

/// Basic CIDR matching for IP addresses.
fn cidr_match(cidr: &str, ip: &str) -> bool {
    // Parse CIDR notation (e.g., "192.168.1.0/24")
    let parts: Vec<&str> = cidr.split('/').collect();
    if parts.len() != 2 {
        return false;
    }

    let network = parts[0];
    let prefix_len: u32 = match parts[1].parse() {
        Ok(p) if p <= 32 => p,
        _ => return false,
    };

    // Parse network IP
    let network_parts: Vec<u8> = network.split('.').filter_map(|s| s.parse().ok()).collect();
    if network_parts.len() != 4 {
        return false;
    }

    // Parse target IP
    let ip_parts: Vec<u8> = ip.split('.').filter_map(|s| s.parse().ok()).collect();
    if ip_parts.len() != 4 {
        return false;
    }

    // Convert to u32
    let network_u32 = u32::from_be_bytes([
        network_parts[0],
        network_parts[1],
        network_parts[2],
        network_parts[3],
    ]);
    let ip_u32 = u32::from_be_bytes([ip_parts[0], ip_parts[1], ip_parts[2], ip_parts[3]]);

    // Create mask and compare
    let mask = if prefix_len == 0 {
        0
    } else {
        !0u32 << (32 - prefix_len)
    };

    (network_u32 & mask) == (ip_u32 & mask)
}
