//! In-memory ban cache for fast connection-time ban checks.
//!
//! Caches K-lines, D-lines, G-lines, and Z-lines from the database for
//! O(n) pattern matching without database queries on every connection.
//!
//! # Architecture
//!
//! - Loaded from database on startup
//! - Updated when admin commands add/remove bans
//! - Checked at connection time before handshake completes
//! - Expired bans are lazily filtered during checks

use crate::db::{Dline, Gline, Kline, Zline};
use dashmap::DashMap;
use slirc_proto::wildcard_match;
use std::net::IpAddr;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

/// Result of a ban check.
#[derive(Debug, Clone)]
pub struct BanResult {
    /// The type of ban that matched.
    pub ban_type: BanType,
    /// The pattern that matched.
    #[allow(dead_code)] // Available for logging/display
    pub pattern: String,
    /// The reason for the ban.
    pub reason: String,
}

/// Type of ban that matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // Traditional IRC naming: K-Line, G-Line, etc.
pub enum BanType {
    /// Z-line: IP ban (no DNS lookup).
    ZLine,
    /// D-line: IP ban.
    DLine,
    /// G-line: Global user@host ban.
    GLine,
    /// K-line: Local user@host ban.
    KLine,
}

impl BanType {
    /// Get the display name for this ban type.
    pub fn name(&self) -> &'static str {
        match self {
            BanType::ZLine => "Z-lined",
            BanType::DLine => "D-lined",
            BanType::GLine => "G-lined",
            BanType::KLine => "K-lined",
        }
    }
}

impl std::fmt::Display for BanType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// In-memory cache of active bans for fast lookup.
///
/// All ban types are stored in DashMaps keyed by their mask pattern.
/// Expiration is checked at lookup time (lazy expiration).
#[derive(Debug)]
pub struct BanCache {
    /// K-lines: user@host local bans.
    klines: DashMap<String, CachedBan>,
    /// D-lines: IP bans.
    dlines: DashMap<String, CachedBan>,
    /// G-lines: user@host global bans.
    glines: DashMap<String, CachedBan>,
    /// Z-lines: IP bans (no DNS).
    zlines: DashMap<String, CachedBan>,
}

/// A cached ban entry with expiration tracking.
#[derive(Debug, Clone)]
struct CachedBan {
    /// The ban mask/pattern.
    mask: String,
    /// Reason for the ban.
    reason: String,
    /// Unix timestamp when ban expires (None = permanent).
    expires_at: Option<i64>,
}

impl CachedBan {
    /// Check if this ban has expired.
    fn is_expired(&self) -> bool {
        if let Some(expires) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            now > expires
        } else {
            false // Permanent ban
        }
    }
}

impl Default for BanCache {
    fn default() -> Self {
        Self::new()
    }
}

impl BanCache {
    /// Create an empty ban cache.
    pub fn new() -> Self {
        Self {
            klines: DashMap::new(),
            dlines: DashMap::new(),
            glines: DashMap::new(),
            zlines: DashMap::new(),
        }
    }

    /// Load bans from database models into the cache.
    ///
    /// Called on startup to populate the cache.
    pub fn load(
        klines: Vec<Kline>,
        dlines: Vec<Dline>,
        glines: Vec<Gline>,
        zlines: Vec<Zline>,
    ) -> Self {
        let cache = Self::new();

        for k in klines {
            cache.klines.insert(
                k.mask.clone(),
                CachedBan {
                    mask: k.mask,
                    reason: k.reason.unwrap_or_else(|| "Banned".to_string()),
                    expires_at: k.expires_at,
                },
            );
        }

        for d in dlines {
            cache.dlines.insert(
                d.mask.clone(),
                CachedBan {
                    mask: d.mask,
                    reason: d.reason.unwrap_or_else(|| "Banned".to_string()),
                    expires_at: d.expires_at,
                },
            );
        }

        for g in glines {
            cache.glines.insert(
                g.mask.clone(),
                CachedBan {
                    mask: g.mask,
                    reason: g.reason.unwrap_or_else(|| "Banned".to_string()),
                    expires_at: g.expires_at,
                },
            );
        }

        for z in zlines {
            cache.zlines.insert(
                z.mask.clone(),
                CachedBan {
                    mask: z.mask,
                    reason: z.reason.unwrap_or_else(|| "Banned".to_string()),
                    expires_at: z.expires_at,
                },
            );
        }

        debug!(
            klines = cache.klines.len(),
            dlines = cache.dlines.len(),
            glines = cache.glines.len(),
            zlines = cache.zlines.len(),
            "Ban cache loaded"
        );

        cache
    }

    /// Check if an IP is banned (Z-line or D-line).
    ///
    /// Called at connection time before any handshake.
    /// Checks Z-lines first (IP ban, skips DNS), then D-lines.
    pub fn check_ip(&self, ip: &IpAddr) -> Option<BanResult> {
        let ip_str = ip.to_string();

        // Check Z-lines first (IP ban, no DNS lookup)
        for entry in self.zlines.iter() {
            let ban = entry.value();
            if ban.is_expired() {
                continue;
            }
            if self.matches_ip_pattern(&ban.mask, &ip_str) {
                return Some(BanResult {
                    ban_type: BanType::ZLine,
                    pattern: ban.mask.clone(),
                    reason: ban.reason.clone(),
                });
            }
        }

        // Check D-lines
        for entry in self.dlines.iter() {
            let ban = entry.value();
            if ban.is_expired() {
                continue;
            }
            if self.matches_ip_pattern(&ban.mask, &ip_str) {
                return Some(BanResult {
                    ban_type: BanType::DLine,
                    pattern: ban.mask.clone(),
                    reason: ban.reason.clone(),
                });
            }
        }

        None
    }

    /// Check if a user@host is banned (G-line or K-line).
    ///
    /// Called after USER command when we have the full user@host.
    pub fn check_user_host(&self, user: &str, host: &str) -> Option<BanResult> {
        let user_host = format!("{}@{}", user, host);

        // Check G-lines first (global)
        for entry in self.glines.iter() {
            let ban = entry.value();
            if ban.is_expired() {
                continue;
            }
            if wildcard_match(&ban.mask, &user_host) {
                return Some(BanResult {
                    ban_type: BanType::GLine,
                    pattern: ban.mask.clone(),
                    reason: ban.reason.clone(),
                });
            }
        }

        // Check K-lines (local)
        for entry in self.klines.iter() {
            let ban = entry.value();
            if ban.is_expired() {
                continue;
            }
            if wildcard_match(&ban.mask, &user_host) {
                return Some(BanResult {
                    ban_type: BanType::KLine,
                    pattern: ban.mask.clone(),
                    reason: ban.reason.clone(),
                });
            }
        }

        None
    }

    /// Add a K-line to the cache.
    pub fn add_kline(&self, mask: String, reason: String, expires_at: Option<i64>) {
        self.klines.insert(
            mask.clone(),
            CachedBan {
                mask,
                reason,
                expires_at,
            },
        );
    }

    /// Add a D-line to the cache.
    pub fn add_dline(&self, mask: String, reason: String, expires_at: Option<i64>) {
        self.dlines.insert(
            mask.clone(),
            CachedBan {
                mask,
                reason,
                expires_at,
            },
        );
    }

    /// Add a G-line to the cache.
    pub fn add_gline(&self, mask: String, reason: String, expires_at: Option<i64>) {
        self.glines.insert(
            mask.clone(),
            CachedBan {
                mask,
                reason,
                expires_at,
            },
        );
    }

    /// Add a Z-line to the cache.
    pub fn add_zline(&self, mask: String, reason: String, expires_at: Option<i64>) {
        self.zlines.insert(
            mask.clone(),
            CachedBan {
                mask,
                reason,
                expires_at,
            },
        );
    }

    /// Remove a K-line from the cache.
    pub fn remove_kline(&self, mask: &str) {
        self.klines.remove(mask);
    }

    /// Remove a D-line from the cache.
    pub fn remove_dline(&self, mask: &str) {
        self.dlines.remove(mask);
    }

    /// Remove a G-line from the cache.
    pub fn remove_gline(&self, mask: &str) {
        self.glines.remove(mask);
    }

    /// Remove a Z-line from the cache.
    pub fn remove_zline(&self, mask: &str) {
        self.zlines.remove(mask);
    }

    /// Prune expired bans from all caches.
    ///
    /// Called periodically by a background task.
    pub fn prune_expired(&self) -> usize {
        let mut removed = 0;

        self.klines.retain(|_, ban| {
            if ban.is_expired() {
                removed += 1;
                false
            } else {
                true
            }
        });

        self.dlines.retain(|_, ban| {
            if ban.is_expired() {
                removed += 1;
                false
            } else {
                true
            }
        });

        self.glines.retain(|_, ban| {
            if ban.is_expired() {
                removed += 1;
                false
            } else {
                true
            }
        });

        self.zlines.retain(|_, ban| {
            if ban.is_expired() {
                removed += 1;
                false
            } else {
                true
            }
        });

        if removed > 0 {
            debug!(count = removed, "Pruned expired bans from cache");
        }

        removed
    }

    /// Match an IP pattern (supports wildcards and CIDR notation).
    fn matches_ip_pattern(&self, pattern: &str, ip: &str) -> bool {
        // Check for CIDR notation
        if pattern.contains('/') {
            return cidr_match(pattern, ip);
        }

        // Otherwise use wildcard matching
        wildcard_match(pattern, ip)
    }
}

/// Match IP against CIDR notation (e.g., "192.168.1.0/24").
fn cidr_match(cidr: &str, ip: &str) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cidr_match() {
        assert!(cidr_match("192.168.1.0/24", "192.168.1.100"));
        assert!(cidr_match("192.168.1.0/24", "192.168.1.1"));
        assert!(!cidr_match("192.168.1.0/24", "192.168.2.1"));
        assert!(cidr_match("10.0.0.0/8", "10.255.255.255"));
        assert!(!cidr_match("10.0.0.0/8", "11.0.0.1"));
    }

    #[test]
    fn test_wildcard_ip_match() {
        let cache = BanCache::new();
        assert!(cache.matches_ip_pattern("192.168.*.*", "192.168.1.100"));
        assert!(cache.matches_ip_pattern("192.168.1.*", "192.168.1.50"));
        assert!(!cache.matches_ip_pattern("192.168.1.*", "192.168.2.50"));
    }

    #[test]
    fn test_ban_expiration() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        // Expired ban
        let expired = CachedBan {
            mask: "*@*".to_string(),
            reason: "Test".to_string(),
            expires_at: Some(now - 3600), // 1 hour ago
        };
        assert!(expired.is_expired());

        // Active ban
        let active = CachedBan {
            mask: "*@*".to_string(),
            reason: "Test".to_string(),
            expires_at: Some(now + 3600), // 1 hour from now
        };
        assert!(!active.is_expired());

        // Permanent ban
        let permanent = CachedBan {
            mask: "*@*".to_string(),
            reason: "Test".to_string(),
            expires_at: None,
        };
        assert!(!permanent.is_expired());
    }
}
