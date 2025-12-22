//! In-memory ban cache for fast connection-time ban checks.
//!
//! Caches K-lines and G-lines from the database for O(n) pattern matching
//! without database queries on every connection.
//!
//! # Architecture
//!
//! - Loaded from database on startup
//! - Updated when admin commands add/remove bans
//! - Checked at connection time before handshake completes
//! - Expired bans are lazily filtered during checks
//!
//! # Note on IP Bans
//!
//! Z-lines and D-lines (IP-based bans) are handled by `IpDenyList` which
//! provides O(1) Roaring Bitmap lookups in the gateway hot path.

use crate::db::{Gline, Kline};
use dashmap::DashMap;
use slirc_proto::wildcard_match;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

/// Result of a ban check.
#[derive(Debug, Clone)]
pub struct BanResult {
    /// The type of ban that matched.
    pub ban_type: BanType,
    /// The reason for the ban.
    pub reason: String,
}

/// Type of ban that matched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)] // Traditional IRC naming: K-Line, G-Line, etc.
pub enum BanType {
    /// G-line: Global user@host ban.
    GLine,
    /// K-line: Local user@host ban.
    KLine,
}

impl BanType {
    /// Get the display name for this ban type.
    pub fn name(&self) -> &'static str {
        match self {
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
/// Stores K-lines and G-lines in DashMaps keyed by their mask pattern.
/// Expiration is checked at lookup time (lazy expiration).
///
/// IP-based bans (Z-lines, D-lines) are handled by `IpDenyList`.
#[derive(Debug)]
pub struct BanCache {
    /// K-lines: user@host local bans.
    klines: DashMap<String, CachedBan>,
    /// G-lines: user@host global bans.
    glines: DashMap<String, CachedBan>,
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
            glines: DashMap::new(),
        }
    }

    /// Load bans from database models into the cache.
    ///
    /// Called on startup to populate the cache.
    /// Only loads K-lines and G-lines; IP bans are handled by IpDenyList.
    pub fn load(klines: Vec<Kline>, glines: Vec<Gline>) -> Self {
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

        debug!(
            klines = cache.klines.len(),
            glines = cache.glines.len(),
            "Ban cache loaded"
        );

        cache
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

    /// Remove a K-line from the cache.
    pub fn remove_kline(&self, mask: &str) {
        self.klines.remove(mask);
    }

    /// Remove a G-line from the cache.
    pub fn remove_gline(&self, mask: &str) {
        self.glines.remove(mask);
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

        self.glines.retain(|_, ban| {
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

    /// Iterate over all G-lines (mask, reason, expires_at).
    ///
    /// Used for BURST to synchronize bans with peers.
    pub fn iter_glines(&self) -> impl Iterator<Item = (String, String, Option<i64>)> + '_ {
        self.glines.iter().filter_map(|entry| {
            let ban = entry.value();
            if ban.is_expired() {
                None
            } else {
                Some((ban.mask.clone(), ban.reason.clone(), ban.expires_at))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn test_user_host_matching() {
        let cache = BanCache::new();
        cache.add_kline("*@*.badhost.com".to_string(), "Bad host".to_string(), None);
        cache.add_gline("baduser@*".to_string(), "Bad user".to_string(), None);

        // Should match K-line
        let result = cache.check_user_host("anyone", "server.badhost.com");
        assert!(result.is_some());
        assert_eq!(result.unwrap().ban_type, BanType::KLine);

        // Should match G-line
        let result = cache.check_user_host("baduser", "anyhost.com");
        assert!(result.is_some());
        assert_eq!(result.unwrap().ban_type, BanType::GLine);

        // Should not match
        let result = cache.check_user_host("gooduser", "goodhost.com");
        assert!(result.is_none());
    }
}
