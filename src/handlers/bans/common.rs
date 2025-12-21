//! Shared ban utilities.
//!
//! Common types and helpers used across ban handlers.

use super::super::Context;
use slirc_proto::wildcard_match;

/// Types of bans for matching purposes.
#[derive(Debug, Clone, Copy)]
pub enum BanType {
    /// K-line: matches user@host
    Kline,
    /// D-line: matches IP (with CIDR support)
    Dline,
    /// G-line: matches user@host (global)
    Gline,
    /// Z-line: matches IP (with CIDR support, global)
    Zline,
    /// R-line: matches realname
    Rline,
}

impl BanType {
    /// Returns the ban type name for quit messages.
    pub fn name(&self) -> &'static str {
        match self {
            BanType::Kline => "K-lined",
            BanType::Dline => "D-lined",
            BanType::Gline => "G-lined",
            BanType::Zline => "Z-lined",
            BanType::Rline => "R-lined",
        }
    }
}

/// Disconnect all users matching a ban pattern.
///
/// Consolidates the disconnect logic for all ban types (K/D/G/Z/R-lines).
/// The matching strategy varies by ban type:
/// - K-line/G-line: Match against `user@host`
/// - D-line/Z-line: Match against IP with CIDR support
/// - R-line: Match against realname
pub async fn disconnect_matching_ban<S>(
    ctx: &Context<'_, S>,
    ban_type: BanType,
    pattern: &str,
    reason: &str,
) -> usize {
    let mut to_disconnect = Vec::with_capacity(4); // Ban typically affects few users

    // Collect matching users
    for entry in ctx.matrix.user_manager.users.iter() {
        let uid = entry.key().clone();
        let user = entry.value().read().await;

        let matches = match ban_type {
            BanType::Kline | BanType::Gline => {
                let user_host = format!("{}@{}", user.user, user.host);
                wildcard_match(pattern, &user_host)
            }
            BanType::Dline | BanType::Zline => {
                wildcard_match(pattern, &user.host) || cidr_match(pattern, &user.host)
            }
            BanType::Rline => wildcard_match(pattern, &user.realname),
        };

        if matches {
            to_disconnect.push(uid);
        }
    }

    // Disconnect matching users
    let quit_reason = format!("{}: {}", ban_type.name(), reason);
    for uid in &to_disconnect {
        ctx.matrix.disconnect_user(uid, &quit_reason).await;
    }

    to_disconnect.len()
}

/// Basic CIDR matching for IP addresses.
pub fn cidr_match(cidr: &str, ip: &str) -> bool {
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
