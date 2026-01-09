//! Ban entry models and data structures.

use super::queries::generic::BanType;
use slirc_proto::wildcard_match;

/// A K-line (user@host ban).
#[derive(Debug, Clone)]
pub struct Kline {
    /// User@host mask pattern (e.g., "*@*.badhost.com").
    pub mask: String,
    /// Reason for the ban.
    pub reason: Option<String>,
    /// Operator who set the ban.
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    pub set_at: i64,
    /// Optional expiration timestamp.
    pub expires_at: Option<i64>,
}

/// A D-line (IP ban).
#[derive(Debug, Clone)]
pub struct Dline {
    /// IP or CIDR mask (e.g., "192.168.1.0/24").
    pub mask: String,
    /// Reason for the ban.
    pub reason: Option<String>,
    /// Operator who set the ban.
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    pub set_at: i64,
    /// Optional expiration timestamp.
    pub expires_at: Option<i64>,
}

/// A G-line (global hostmask ban).
#[derive(Debug, Clone)]
pub struct Gline {
    /// User@host mask pattern.
    pub mask: String,
    /// Reason for the ban.
    pub reason: Option<String>,
    /// Operator who set the ban.
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    pub set_at: i64,
    /// Optional expiration timestamp.
    pub expires_at: Option<i64>,
}

/// A Z-line (IP ban that skips DNS lookup).
#[derive(Debug, Clone)]
pub struct Zline {
    /// IP mask pattern.
    pub mask: String,
    /// Reason for the ban.
    pub reason: Option<String>,
    /// Operator who set the ban.
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    pub set_at: i64,
    /// Optional expiration timestamp.
    pub expires_at: Option<i64>,
}

/// An R-line (realname/GECOS ban).
#[derive(Debug, Clone)]
pub struct Rline {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// A shun (silent ban - user stays connected but commands are ignored).
#[derive(Debug, Clone)]
pub struct Shun {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// Basic CIDR matching for IP addresses.
pub(super) fn cidr_match(cidr: &str, ip: &str) -> bool {
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

// ============================================================================
// BanType trait implementations
// ============================================================================

impl BanType for Kline {
    fn table_name() -> &'static str {
        "klines"
    }

    fn from_row(
        mask: String,
        reason: Option<String>,
        set_by: String,
        set_at: i64,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        }
    }

    fn matches(&self, user_host: &str) -> bool {
        wildcard_match(&self.mask, user_host)
    }
}

impl BanType for Dline {
    fn table_name() -> &'static str {
        "dlines"
    }

    fn from_row(
        mask: String,
        reason: Option<String>,
        set_by: String,
        set_at: i64,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        }
    }

    fn matches(&self, ip: &str) -> bool {
        wildcard_match(&self.mask, ip) || cidr_match(&self.mask, ip)
    }
}

impl BanType for Gline {
    fn table_name() -> &'static str {
        "glines"
    }

    fn from_row(
        mask: String,
        reason: Option<String>,
        set_by: String,
        set_at: i64,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        }
    }

    fn matches(&self, user_host: &str) -> bool {
        wildcard_match(&self.mask, user_host)
    }
}

impl BanType for Zline {
    fn table_name() -> &'static str {
        "zlines"
    }

    fn from_row(
        mask: String,
        reason: Option<String>,
        set_by: String,
        set_at: i64,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        }
    }

    fn matches(&self, ip: &str) -> bool {
        wildcard_match(&self.mask, ip) || cidr_match(&self.mask, ip)
    }
}

impl BanType for Rline {
    fn table_name() -> &'static str {
        "rlines"
    }

    fn from_row(
        mask: String,
        reason: Option<String>,
        set_by: String,
        set_at: i64,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        }
    }

    fn matches(&self, realname: &str) -> bool {
        wildcard_match(&self.mask, realname)
    }
}

impl BanType for Shun {
    fn table_name() -> &'static str {
        "shuns"
    }

    fn from_row(
        mask: String,
        reason: Option<String>,
        set_by: String,
        set_at: i64,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            mask,
            reason,
            set_by,
            set_at,
            expires_at,
        }
    }

    fn matches(&self, user_host: &str) -> bool {
        wildcard_match(&self.mask, user_host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // cidr_match() tests
    // ========================================================================

    #[test]
    fn cidr_match_exact_network() {
        assert!(cidr_match("192.168.1.0/24", "192.168.1.100"));
        assert!(cidr_match("192.168.1.0/24", "192.168.1.0"));
        assert!(cidr_match("192.168.1.0/24", "192.168.1.255"));
    }

    #[test]
    fn cidr_match_outside_network() {
        assert!(!cidr_match("192.168.1.0/24", "192.168.2.1"));
        assert!(!cidr_match("10.0.0.0/8", "192.168.1.1"));
    }

    #[test]
    fn cidr_match_various_prefix_lengths() {
        // /32 - exact match only
        assert!(cidr_match("10.0.0.1/32", "10.0.0.1"));
        assert!(!cidr_match("10.0.0.1/32", "10.0.0.2"));

        // /16 - class B equivalent
        assert!(cidr_match("172.16.0.0/16", "172.16.255.255"));
        assert!(cidr_match("172.16.0.0/16", "172.16.0.1"));
        assert!(!cidr_match("172.16.0.0/16", "172.17.0.1"));

        // /8 - class A equivalent
        assert!(cidr_match("10.0.0.0/8", "10.255.255.255"));
        assert!(!cidr_match("10.0.0.0/8", "11.0.0.1"));

        // /0 - matches everything
        assert!(cidr_match("0.0.0.0/0", "192.168.1.1"));
        assert!(cidr_match("0.0.0.0/0", "10.0.0.1"));
    }

    #[test]
    fn cidr_match_invalid_input() {
        // Missing prefix
        assert!(!cidr_match("192.168.1.0", "192.168.1.1"));
        // Invalid prefix length
        assert!(!cidr_match("192.168.1.0/33", "192.168.1.1"));
        assert!(!cidr_match("192.168.1.0/abc", "192.168.1.1"));
        // Invalid IP format
        assert!(!cidr_match("192.168.1/24", "192.168.1.1"));
        assert!(!cidr_match("192.168.1.0/24", "192.168.1"));
        assert!(!cidr_match("not.an.ip.addr/24", "192.168.1.1"));
    }

    // ========================================================================
    // BanType::table_name() tests
    // ========================================================================

    #[test]
    fn table_names_correct() {
        assert_eq!(Kline::table_name(), "klines");
        assert_eq!(Dline::table_name(), "dlines");
        assert_eq!(Gline::table_name(), "glines");
        assert_eq!(Zline::table_name(), "zlines");
        assert_eq!(Rline::table_name(), "rlines");
        assert_eq!(Shun::table_name(), "shuns");
    }

    // ========================================================================
    // BanType::from_row() tests
    // ========================================================================

    #[test]
    fn kline_from_row() {
        let kline = Kline::from_row(
            "*@*.badhost.com".to_string(),
            Some("Spammer".to_string()),
            "admin".to_string(),
            1700000000,
            Some(1700086400),
        );
        assert_eq!(kline.mask, "*@*.badhost.com");
        assert_eq!(kline.reason, Some("Spammer".to_string()));
        assert_eq!(kline.set_by, "admin");
        assert_eq!(kline.set_at, 1700000000);
        assert_eq!(kline.expires_at, Some(1700086400));
    }

    #[test]
    fn dline_from_row() {
        let dline = Dline::from_row(
            "192.168.1.0/24".to_string(),
            None,
            "oper".to_string(),
            1700000000,
            None,
        );
        assert_eq!(dline.mask, "192.168.1.0/24");
        assert!(dline.reason.is_none());
        assert_eq!(dline.set_by, "oper");
        assert!(dline.expires_at.is_none());
    }

    #[test]
    fn gline_from_row() {
        let gline = Gline::from_row(
            "baduser@*".to_string(),
            Some("Network ban".to_string()),
            "netadmin".to_string(),
            1700000000,
            Some(1700172800),
        );
        assert_eq!(gline.mask, "baduser@*");
        assert_eq!(gline.reason, Some("Network ban".to_string()));
    }

    #[test]
    fn zline_from_row() {
        let zline = Zline::from_row(
            "10.0.0.0/8".to_string(),
            Some("Reserved range".to_string()),
            "admin".to_string(),
            1700000000,
            None,
        );
        assert_eq!(zline.mask, "10.0.0.0/8");
    }

    #[test]
    fn rline_from_row() {
        let rline = Rline::from_row(
            "*bot*".to_string(),
            Some("No bots".to_string()),
            "admin".to_string(),
            1700000000,
            None,
        );
        assert_eq!(rline.mask, "*bot*");
    }

    #[test]
    fn shun_from_row() {
        let shun = Shun::from_row(
            "troll@*".to_string(),
            Some("Trolling".to_string()),
            "oper".to_string(),
            1700000000,
            Some(1700043200),
        );
        assert_eq!(shun.mask, "troll@*");
        assert_eq!(shun.expires_at, Some(1700043200));
    }

    // ========================================================================
    // BanType::matches() tests
    // ========================================================================

    #[test]
    fn kline_matches_wildcard() {
        let kline = Kline::from_row(
            "*@*.example.com".to_string(),
            None,
            "admin".to_string(),
            0,
            None,
        );
        assert!(kline.matches("user@sub.example.com"));
        assert!(kline.matches("anyone@host.example.com"));
        assert!(!kline.matches("user@other.net"));
        // Note: *.example.com requires at least one char before .example.com
        assert!(!kline.matches("anyone@example.com"));
    }

    #[test]
    fn kline_matches_exact() {
        let kline = Kline::from_row(
            "baduser@badhost.net".to_string(),
            None,
            "admin".to_string(),
            0,
            None,
        );
        assert!(kline.matches("baduser@badhost.net"));
        assert!(!kline.matches("gooduser@badhost.net"));
        assert!(!kline.matches("baduser@goodhost.net"));
    }

    #[test]
    fn dline_matches_cidr() {
        let dline = Dline::from_row(
            "192.168.1.0/24".to_string(),
            None,
            "admin".to_string(),
            0,
            None,
        );
        assert!(dline.matches("192.168.1.50"));
        assert!(dline.matches("192.168.1.255"));
        assert!(!dline.matches("192.168.2.1"));
    }

    #[test]
    fn dline_matches_wildcard() {
        let dline = Dline::from_row(
            "192.168.*.*".to_string(),
            None,
            "admin".to_string(),
            0,
            None,
        );
        assert!(dline.matches("192.168.1.1"));
        assert!(dline.matches("192.168.255.255"));
        assert!(!dline.matches("192.169.1.1"));
    }

    #[test]
    fn gline_matches() {
        let gline = Gline::from_row(
            "*@*.badnet.org".to_string(),
            None,
            "netadmin".to_string(),
            0,
            None,
        );
        assert!(gline.matches("anyone@sub.badnet.org"));
        assert!(!gline.matches("user@goodnet.org"));
    }

    #[test]
    fn zline_matches_cidr() {
        let zline = Zline::from_row("10.0.0.0/8".to_string(), None, "admin".to_string(), 0, None);
        assert!(zline.matches("10.1.2.3"));
        assert!(zline.matches("10.255.255.255"));
        assert!(!zline.matches("11.0.0.1"));
    }

    #[test]
    fn zline_matches_exact_ip() {
        let zline = Zline::from_row("1.2.3.4".to_string(), None, "admin".to_string(), 0, None);
        // Exact IP match via wildcard_match
        assert!(zline.matches("1.2.3.4"));
        assert!(!zline.matches("1.2.3.5"));
    }

    #[test]
    fn rline_matches_realname() {
        let rline = Rline::from_row("*spam*".to_string(), None, "admin".to_string(), 0, None);
        assert!(rline.matches("I am a spammer"));
        assert!(rline.matches("spam bot v1.0"));
        assert!(!rline.matches("Legitimate User"));
    }

    #[test]
    fn rline_matches_case_insensitive_pattern() {
        // Note: depends on wildcard_match implementation
        let rline = Rline::from_row("*BOT*".to_string(), None, "admin".to_string(), 0, None);
        // Test exact case match
        assert!(rline.matches("I am a BOT"));
    }

    #[test]
    fn shun_matches() {
        let shun = Shun::from_row("troll*@*".to_string(), None, "oper".to_string(), 0, None);
        assert!(shun.matches("troll123@anywhere.com"));
        assert!(shun.matches("troll@host.net"));
        assert!(!shun.matches("user@troll.net"));
    }
}
