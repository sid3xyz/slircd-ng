//! Z-line (global IP ban, skips DNS) handlers.

use super::super::common::{BanType, disconnect_matching_ban};
use crate::handlers::{err_needmoreparams, require_oper, server_notice, Context, Handler, HandlerResult};
use async_trait::async_trait;
use ipnet::IpNet;
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
                tracing::error!(error = %e, "Failed to add Z-line to IP deny list");
            }
        } else {
            tracing::warn!(ip = %ip, "Z-line IP could not be parsed as IP/CIDR");
        }

        // Store Z-line in database (audit trail)
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

        // Remove Z-line from database
        let db_removed = match ctx.db.bans().remove_zline(ip).await {
            Ok(r) => r,
            Err(e) => {
                tracing::error!(error = %e, "Failed to remove Z-line from database");
                false
            }
        };

        let removed = deny_removed || db_removed;
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
