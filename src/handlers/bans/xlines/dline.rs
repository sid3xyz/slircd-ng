//! D-line (local IP ban) handlers.

use super::super::common::{BanType, disconnect_matching_ban};
use crate::handlers::{err_needmoreparams, require_oper, server_notice, Context, Handler, HandlerResult};
use async_trait::async_trait;
use ipnet::IpNet;
use slirc_proto::MessageRef;

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

        // Parse IP/CIDR for the high-performance deny list
        let ip_net: Option<IpNet> = ip.parse().ok().or_else(|| {
            // Try parsing as single IP and convert to /32 or /128
            ip.parse::<std::net::IpAddr>().ok().map(|addr| match addr {
                std::net::IpAddr::V4(v4) => IpNet::V4(ipnet::Ipv4Net::new(v4, 32).expect("prefix 32 is valid")),
                std::net::IpAddr::V6(v6) => IpNet::V6(ipnet::Ipv6Net::new(v6, 128).expect("prefix 128 is valid")),
            })
        });

        // Add to high-performance IP deny list (Roaring Bitmap)
        if let Some(net) = ip_net {
            if let Ok(mut deny_list) = ctx.matrix.ip_deny_list.write()
                && let Err(e) = deny_list.add_ban(net, reason.to_string(), None, nick.clone())
            {
                tracing::error!(error = %e, "Failed to add D-line to IP deny list");
            }
        } else {
            tracing::warn!(ip = %ip, "D-line IP could not be parsed as IP/CIDR");
        }

        // Store D-line in database (audit trail)
        if let Err(e) = ctx.db.bans().add_dline(ip, Some(reason), &nick, None).await {
            tracing::error!(error = %e, "Failed to add D-line to database");
        }

        // Update legacy ban cache for backward compatibility
        ctx.matrix.ban_cache.add_dline(
            ip.to_string(),
            reason.to_string(),
            None, // No expiration for now
        );

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

        // Parse IP/CIDR for the high-performance deny list
        let ip_net: Option<IpNet> = ip.parse().ok().or_else(|| {
            ip.parse::<std::net::IpAddr>().ok().map(|addr| match addr {
                std::net::IpAddr::V4(v4) => IpNet::V4(ipnet::Ipv4Net::new(v4, 32).expect("prefix 32 is valid")),
                std::net::IpAddr::V6(v6) => IpNet::V6(ipnet::Ipv6Net::new(v6, 128).expect("prefix 128 is valid")),
            })
        });

        // Remove from high-performance IP deny list
        let deny_removed = if let Some(net) = ip_net {
            if let Ok(mut deny_list) = ctx.matrix.ip_deny_list.write() {
                deny_list.remove_ban(net).unwrap_or(false)
            } else {
                false
            }
        } else {
            false
        };

        // Remove D-line from database
        let db_removed = match ctx.db.bans().remove_dline(ip).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove D-line from database");
                false
            }
        };

        // Remove from legacy ban cache
        ctx.matrix.ban_cache.remove_dline(ip);

        let removed = deny_removed || db_removed;
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
