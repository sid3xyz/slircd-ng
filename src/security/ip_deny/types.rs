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
