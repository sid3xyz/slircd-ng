//! Ban entry models and data structures.

/// A K-line (user@host ban).
#[derive(Debug, Clone)]
pub struct Kline {
    /// User@host mask pattern (e.g., "*@*.badhost.com").
    pub mask: String,
    /// Reason for the ban.
    pub reason: Option<String>,
    /// Operator who set the ban.
    #[allow(dead_code)] // Used by admin STATS command
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    #[allow(dead_code)] // Used by admin STATS command
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
    #[allow(dead_code)] // Used by admin STATS command
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    #[allow(dead_code)] // Used by admin STATS command
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
    #[allow(dead_code)] // Used by admin STATS command
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    #[allow(dead_code)] // Used by admin STATS command
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
    #[allow(dead_code)] // Used by admin STATS command
    pub set_by: String,
    /// Unix timestamp when the ban was set.
    #[allow(dead_code)] // Used by admin STATS command
    pub set_at: i64,
    /// Optional expiration timestamp.
    pub expires_at: Option<i64>,
}

/// An R-line (realname/GECOS ban).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by admin commands in Phase 3b
pub struct Rline {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// A shun (silent ban - user stays connected but commands are ignored).
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields used by admin for stats/inspection
pub struct Shun {
    pub mask: String,
    pub reason: Option<String>,
    pub set_by: String,
    pub set_at: i64,
    pub expires_at: Option<i64>,
}

/// Basic CIDR matching for IP addresses.
#[allow(dead_code)] // Used by matches_dline
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
