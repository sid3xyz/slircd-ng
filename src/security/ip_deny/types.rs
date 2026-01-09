//! Type definitions for IP deny list.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Default filename for MessagePack persistence.
pub(super) const DEFAULT_PERSIST_PATH: &str = "ip_bans.msgpack";

/// Metadata for a banned IP or CIDR.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BanMetadata {
    /// Human-readable reason for the ban.
    pub reason: String,
    /// Optional expiration time (Unix timestamp). None = permanent.
    pub expiry: Option<u64>,
    /// Operator or system that added the ban.
    pub added_by: String,
    /// When the ban was added (Unix timestamp).
    pub added_at: u64,
}

impl BanMetadata {
    /// Create new metadata with current timestamp.
    pub fn new(reason: String, duration: Option<Duration>, added_by: String) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let expiry = duration.map(|d| now + d.as_secs());

        Self {
            reason,
            expiry,
            added_by,
            added_at: now,
        }
    }

    /// Check if this ban has expired.
    #[inline]
    pub fn is_expired(&self) -> bool {
        if let Some(expiry) = self.expiry {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            now > expiry
        } else {
            false // Permanent ban
        }
    }
}

/// Serializable state for persistence.
#[derive(Debug, Serialize, Deserialize)]
pub(super) struct PersistentState {
    /// IPv4 single-address bans (as u32 values).
    pub ipv4_singles: Vec<u32>,
    /// IPv4 CIDR bans (stored as "ip/prefix" strings).
    pub ipv4_cidrs: Vec<String>,
    /// IPv6 CIDR bans (stored as "ip/prefix" strings).
    pub ipv6_cidrs: Vec<String>,
    /// Metadata keyed by IP/CIDR string.
    pub metadata: HashMap<String, BanMetadata>,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // BanMetadata::new tests
    // ========================================================================

    #[test]
    fn ban_metadata_new_permanent() {
        let meta = BanMetadata::new("Spammer".to_string(), None, "admin".to_string());
        assert_eq!(meta.reason, "Spammer");
        assert_eq!(meta.added_by, "admin");
        assert!(meta.expiry.is_none());
        assert!(meta.added_at > 0);
    }

    #[test]
    fn ban_metadata_new_with_duration() {
        let duration = Duration::from_secs(3600);
        let meta = BanMetadata::new("Temp ban".to_string(), Some(duration), "oper".to_string());
        assert_eq!(meta.reason, "Temp ban");
        assert!(meta.expiry.is_some());
        // Expiry should be approximately now + 3600
        let expiry = meta.expiry.unwrap();
        assert!(expiry > meta.added_at);
        assert!(expiry <= meta.added_at + 3601); // Allow 1 second tolerance
    }

    // ========================================================================
    // BanMetadata::is_expired tests
    // ========================================================================

    #[test]
    fn permanent_ban_never_expires() {
        let meta = BanMetadata::new("Permanent".to_string(), None, "admin".to_string());
        assert!(!meta.is_expired());
    }

    #[test]
    fn future_ban_not_expired() {
        let duration = Duration::from_secs(3600); // 1 hour
        let meta = BanMetadata::new("Future".to_string(), Some(duration), "admin".to_string());
        assert!(!meta.is_expired());
    }

    #[test]
    fn past_ban_is_expired() {
        let meta = BanMetadata {
            reason: "Expired".to_string(),
            expiry: Some(1), // Unix timestamp 1 = 1970, definitely expired
            added_by: "admin".to_string(),
            added_at: 0,
        };
        assert!(meta.is_expired());
    }

    #[test]
    fn zero_duration_expires_immediately() {
        // A ban with 0 duration should expire almost immediately
        let meta = BanMetadata::new(
            "Zero dur".to_string(),
            Some(Duration::ZERO),
            "admin".to_string(),
        );
        // This is tricky - it expires at added_at + 0, so it depends on timing
        // The expiry equals added_at, and is_expired checks if now > expiry
        // So it won't be expired immediately, but will be expired after 1 second
        assert!(meta.expiry.is_some());
        assert_eq!(meta.expiry.unwrap(), meta.added_at);
    }
}
