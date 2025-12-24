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

/// Parse a duration string into seconds.
///
/// Supports formats like:
/// - `30` or `30s` - 30 seconds
/// - `5m` - 5 minutes
/// - `2h` - 2 hours
/// - `1d` - 1 day
/// - `1w` - 1 week
/// - `1d2h30m` - combined
///
/// Returns `None` for permanent bans (0 or empty string).
pub fn parse_duration(s: &str) -> Option<i64> {
    if s.is_empty() || s == "0" {
        return None; // Permanent
    }

    // Try parsing as plain seconds first
    if let Ok(secs) = s.parse::<i64>() {
        return if secs <= 0 { None } else { Some(secs) };
    }

    let mut total_seconds: i64 = 0;
    let mut current_num = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() {
            current_num.push(c);
        } else {
            let num: i64 = current_num.parse().unwrap_or(0);
            current_num.clear();

            let multiplier = match c {
                's' | 'S' => 1,
                'm' | 'M' => 60,
                'h' | 'H' => 3600,
                'd' | 'D' => 86400,
                'w' | 'W' => 604800,
                _ => return None, // Invalid format
            };

            total_seconds += num * multiplier;
        }
    }

    // Handle trailing number (interpreted as seconds)
    if !current_num.is_empty()
        && let Ok(num) = current_num.parse::<i64>()
    {
        total_seconds += num;
    }

    if total_seconds <= 0 {
        None
    } else {
        Some(total_seconds)
    }
}

/// Format a duration in seconds to a human-readable string.
pub fn format_duration(seconds: i64) -> String {
    if seconds <= 0 {
        return "permanent".to_string();
    }

    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{}s", secs));
    }

    parts.join("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(parse_duration("0"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("30"), Some(30));
        assert_eq!(parse_duration("30s"), Some(30));
        assert_eq!(parse_duration("5m"), Some(300));
        assert_eq!(parse_duration("2h"), Some(7200));
        assert_eq!(parse_duration("1d"), Some(86400));
        assert_eq!(parse_duration("1w"), Some(604800));
        assert_eq!(parse_duration("1d2h30m"), Some(86400 + 7200 + 1800));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "permanent");
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(300), "5m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90061), "1d1h1m1s");
    }

    // ─────────────────────────────────────────────────────────────────────────
    // cidr_match tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_cidr_match_exact() {
        // /32 should match only the exact IP
        assert!(cidr_match("192.168.1.100/32", "192.168.1.100"));
        assert!(!cidr_match("192.168.1.100/32", "192.168.1.101"));
    }

    #[test]
    fn test_cidr_match_24() {
        // /24 should match the entire Class C range
        assert!(cidr_match("192.168.1.0/24", "192.168.1.0"));
        assert!(cidr_match("192.168.1.0/24", "192.168.1.100"));
        assert!(cidr_match("192.168.1.0/24", "192.168.1.255"));
        assert!(!cidr_match("192.168.1.0/24", "192.168.2.0"));
    }

    #[test]
    fn test_cidr_match_16() {
        // /16 should match the entire Class B range
        assert!(cidr_match("192.168.0.0/16", "192.168.0.1"));
        assert!(cidr_match("192.168.0.0/16", "192.168.255.255"));
        assert!(!cidr_match("192.168.0.0/16", "192.169.0.0"));
    }

    #[test]
    fn test_cidr_match_8() {
        // /8 should match the entire Class A range
        assert!(cidr_match("10.0.0.0/8", "10.0.0.1"));
        assert!(cidr_match("10.0.0.0/8", "10.255.255.255"));
        assert!(!cidr_match("10.0.0.0/8", "11.0.0.0"));
    }

    #[test]
    fn test_cidr_match_0() {
        // /0 should match any IP
        assert!(cidr_match("0.0.0.0/0", "192.168.1.100"));
        assert!(cidr_match("0.0.0.0/0", "10.0.0.1"));
        assert!(cidr_match("0.0.0.0/0", "255.255.255.255"));
    }

    #[test]
    fn test_cidr_no_match() {
        // IPs outside the range should not match
        assert!(!cidr_match("192.168.1.0/24", "192.168.2.100"));
        assert!(!cidr_match("10.0.0.0/24", "10.0.1.1"));
    }

    #[test]
    fn test_cidr_invalid_format() {
        // Invalid CIDR notation should return false
        assert!(!cidr_match("not-a-cidr", "192.168.1.100"));
        assert!(!cidr_match("192.168.1.0", "192.168.1.100")); // Missing prefix
        assert!(!cidr_match("", "192.168.1.100"));
    }

    #[test]
    fn test_cidr_invalid_prefix() {
        // Prefix > 32 should return false
        assert!(!cidr_match("192.168.1.0/33", "192.168.1.100"));
        assert!(!cidr_match("192.168.1.0/99", "192.168.1.100"));
        assert!(!cidr_match("192.168.1.0/-1", "192.168.1.100"));
    }

    #[test]
    fn test_cidr_invalid_ip() {
        // Invalid target IP should return false
        assert!(!cidr_match("192.168.1.0/24", "not.an.ip"));
        assert!(!cidr_match("192.168.1.0/24", ""));
        assert!(!cidr_match("192.168.1.0/24", "192.168.1")); // Incomplete
    }

    #[test]
    fn test_cidr_invalid_network() {
        // Invalid network address should return false
        assert!(!cidr_match("not.valid/24", "192.168.1.100"));
        assert!(!cidr_match("192.168/24", "192.168.1.100")); // Incomplete
    }

    // ─────────────────────────────────────────────────────────────────────────
    // BanType::name tests
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn test_ban_type_names() {
        assert_eq!(BanType::Kline.name(), "K-lined");
        assert_eq!(BanType::Dline.name(), "D-lined");
        assert_eq!(BanType::Gline.name(), "G-lined");
        assert_eq!(BanType::Zline.name(), "Z-lined");
        assert_eq!(BanType::Rline.name(), "R-lined");
    }
}
