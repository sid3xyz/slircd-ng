//! High-performance IP deny list using Roaring Bitmaps.
//!
//! This module provides nanosecond-scale IP rejection for the connection
//! accept loop, separating high-frequency network defense from SQL I/O.
//!
//! # Architecture
//!
//! - **IPv4**: Uses `RoaringBitmap` for O(1) existence checks
//! - **IPv6**: Uses `Vec<Ipv6Net>` for CIDR scanning (less common)
//! - **Metadata**: `HashMap` stores ban reasons, expiry, and audit info
//! - **Persistence**: MessagePack with atomic writes (temp file + rename)
//!
//! # Hot Path
//!
//! The `check_ip()` method is designed for the gateway accept loop:
//! 1. Acquire read lock on `IpDenyList`
//! 2. Check bitmap/vec (nanoseconds)
//! 3. If hit, check expiry in metadata
//! 4. Return reason or None
//!
//! Expired bans are lazily ignored; a background task handles cleanup.

// Phase 1: IpDenyList engine is implemented but not yet integrated.
// Integration happens in Phase 2-4 (Matrix, Gateway, Handlers).

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use roaring::RoaringBitmap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

/// Default filename for MessagePack persistence.
const DEFAULT_PERSIST_PATH: &str = "ip_bans.msgpack";

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
struct PersistentState {
    /// IPv4 single-address bans (as u32 values).
    ipv4_singles: Vec<u32>,
    /// IPv4 CIDR bans (stored as "ip/prefix" strings).
    ipv4_cidrs: Vec<String>,
    /// IPv6 CIDR bans (stored as "ip/prefix" strings).
    ipv6_cidrs: Vec<String>,
    /// Metadata keyed by IP/CIDR string.
    metadata: HashMap<String, BanMetadata>,
}

/// High-performance IP deny list engine.
///
/// Uses Roaring Bitmaps for IPv4 single addresses (nanosecond lookups)
/// and CIDR vectors for network ranges.
#[derive(Debug)]
pub struct IpDenyList {
    /// Bitmap for single IPv4 addresses. Key is the u32 representation.
    ipv4_bitmap: RoaringBitmap,
    /// IPv4 CIDR ranges (networks, not single IPs).
    ipv4_cidrs: Vec<Ipv4Net>,
    /// IPv6 CIDR ranges.
    ipv6_cidrs: Vec<Ipv6Net>,
    /// Metadata for all bans. Key is the canonical IP/CIDR string.
    metadata: HashMap<String, BanMetadata>,
    /// Path for MessagePack persistence.
    persist_path: PathBuf,
}

impl IpDenyList {
    /// Create an empty deny list with default persistence path.
    pub fn new() -> Self {
        Self::with_path(DEFAULT_PERSIST_PATH)
    }

    /// Create an empty deny list with custom persistence path.
    pub fn with_path<P: AsRef<Path>>(path: P) -> Self {
        Self {
            ipv4_bitmap: RoaringBitmap::new(),
            ipv4_cidrs: Vec::new(),
            ipv6_cidrs: Vec::new(),
            metadata: HashMap::new(),
            persist_path: path.as_ref().to_path_buf(),
        }
    }

    /// Load deny list from disk. Returns empty list if file doesn't exist.
    pub fn load<P: AsRef<Path>>(path: P) -> Self {
        let path = path.as_ref();

        if !path.exists() {
            info!(path = %path.display(), "IP deny list file not found, starting empty");
            return Self::with_path(path);
        }

        match Self::load_from_file(path) {
            Ok(list) => {
                info!(
                    path = %path.display(),
                    ipv4_singles = list.ipv4_bitmap.len(),
                    ipv4_cidrs = list.ipv4_cidrs.len(),
                    ipv6_cidrs = list.ipv6_cidrs.len(),
                    total_bans = list.metadata.len(),
                    "IP deny list loaded"
                );
                list
            }
            Err(e) => {
                error!(path = %path.display(), error = %e, "Failed to load IP deny list, starting empty");
                Self::with_path(path)
            }
        }
    }

    /// Internal: Load from MessagePack file.
    fn load_from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let reader = BufReader::new(file);
        let state: PersistentState = rmp_serde::from_read(reader)?;

        let mut list = Self::with_path(path);

        // Restore IPv4 singles
        for ip_u32 in state.ipv4_singles {
            list.ipv4_bitmap.insert(ip_u32);
        }

        // Restore IPv4 CIDRs
        for cidr_str in state.ipv4_cidrs {
            if let Ok(net) = cidr_str.parse::<Ipv4Net>() {
                list.ipv4_cidrs.push(net);
            } else {
                warn!(cidr = %cidr_str, "Failed to parse IPv4 CIDR from persistence");
            }
        }

        // Restore IPv6 CIDRs
        for cidr_str in state.ipv6_cidrs {
            if let Ok(net) = cidr_str.parse::<Ipv6Net>() {
                list.ipv6_cidrs.push(net);
            } else {
                warn!(cidr = %cidr_str, "Failed to parse IPv6 CIDR from persistence");
            }
        }

        // Restore metadata
        list.metadata = state.metadata;

        Ok(list)
    }

    /// **HOT PATH**: Check if an IP address is banned.
    ///
    /// Returns `Some(reason)` if banned (and not expired), `None` otherwise.
    /// Designed for nanosecond-scale performance in the accept loop.
    #[inline]
    pub fn check_ip(&self, ip: &IpAddr) -> Option<String> {
        match ip {
            IpAddr::V4(ipv4) => self.check_ipv4(ipv4),
            IpAddr::V6(ipv6) => self.check_ipv6(ipv6),
        }
    }

    /// Check IPv4 address against bitmap and CIDR list.
    #[inline]
    fn check_ipv4(&self, ip: &Ipv4Addr) -> Option<String> {
        let ip_u32 = u32::from_be_bytes(ip.octets());
        let ip_str = ip.to_string();

        // Fast bitmap check for single IPs
        if self.ipv4_bitmap.contains(ip_u32)
            && let Some(meta) = self.metadata.get(&ip_str)
            && !meta.is_expired()
        {
            return Some(meta.reason.clone());
        }

        // CIDR scan (less common, but necessary for ranges)
        for net in &self.ipv4_cidrs {
            if net.contains(ip)
                && let Some(meta) = self.metadata.get(&net.to_string())
                && !meta.is_expired()
            {
                return Some(meta.reason.clone());
            }
        }

        None
    }

    /// Check IPv6 address against CIDR list.
    #[inline]
    fn check_ipv6(&self, ip: &Ipv6Addr) -> Option<String> {
        for net in &self.ipv6_cidrs {
            if net.contains(ip)
                && let Some(meta) = self.metadata.get(&net.to_string())
                && !meta.is_expired()
            {
                return Some(meta.reason.clone());
            }
        }

        // Also check single IPv6 addresses
        let ip_str = ip.to_string();
        if let Some(meta) = self.metadata.get(&ip_str)
            && !meta.is_expired()
        {
            return Some(meta.reason.clone());
        }

        None
    }

    /// Add a ban for an IP or CIDR range.
    ///
    /// # Arguments
    /// * `net` - IP network (single address uses /32 or /128)
    /// * `reason` - Human-readable ban reason
    /// * `duration` - Optional ban duration (None = permanent)
    /// * `added_by` - Operator or system adding the ban
    ///
    /// Automatically triggers persistence to disk.
    pub fn add_ban(
        &mut self,
        net: IpNet,
        reason: String,
        duration: Option<Duration>,
        added_by: String,
    ) -> Result<(), std::io::Error> {
        let key = net.to_string();
        let meta = BanMetadata::new(reason.clone(), duration, added_by.clone());

        match net {
            IpNet::V4(v4net) => {
                if v4net.prefix_len() == 32 {
                    // Single IP - use bitmap
                    let ip_u32 = u32::from_be_bytes(v4net.addr().octets());
                    self.ipv4_bitmap.insert(ip_u32);
                    // Store metadata under just the IP, not /32
                    let ip_key = v4net.addr().to_string();
                    self.metadata.insert(ip_key, meta);
                } else {
                    // CIDR range
                    if !self.ipv4_cidrs.contains(&v4net) {
                        self.ipv4_cidrs.push(v4net);
                    }
                    self.metadata.insert(key, meta);
                }
            }
            IpNet::V6(v6net) => {
                if v6net.prefix_len() == 128 {
                    // Single IPv6 - store in metadata only (no bitmap for IPv6)
                    let ip_key = v6net.addr().to_string();
                    self.metadata.insert(ip_key, meta);
                } else {
                    if !self.ipv6_cidrs.contains(&v6net) {
                        self.ipv6_cidrs.push(v6net);
                    }
                    self.metadata.insert(key, meta);
                }
            }
        }

        debug!(
            ban = %net,
            reason = %reason,
            by = %added_by,
            duration = ?duration,
            "Added IP ban"
        );

        self.save()
    }

    /// Add a ban from an IP address (treated as /32 or /128).
    #[allow(dead_code)] // Available for future admin commands
    pub fn add_ban_ip(
        &mut self,
        ip: IpAddr,
        reason: String,
        duration: Option<Duration>,
        added_by: String,
    ) -> Result<(), std::io::Error> {
        let net = match ip {
            IpAddr::V4(v4) => IpNet::V4(Ipv4Net::new(v4, 32).expect("prefix 32 is valid")),
            IpAddr::V6(v6) => IpNet::V6(Ipv6Net::new(v6, 128).expect("prefix 128 is valid")),
        };
        self.add_ban(net, reason, duration, added_by)
    }

    /// Remove a ban for an IP or CIDR range.
    ///
    /// Automatically triggers persistence to disk.
    pub fn remove_ban(&mut self, net: IpNet) -> Result<bool, std::io::Error> {
        let removed = match net {
            IpNet::V4(v4net) => {
                if v4net.prefix_len() == 32 {
                    // Single IP
                    let ip_u32 = u32::from_be_bytes(v4net.addr().octets());
                    let bitmap_removed = self.ipv4_bitmap.remove(ip_u32);
                    let ip_key = v4net.addr().to_string();
                    let meta_removed = self.metadata.remove(&ip_key).is_some();
                    bitmap_removed || meta_removed
                } else {
                    // CIDR range
                    let key = net.to_string();
                    let cidr_removed = self.ipv4_cidrs.iter().position(|n| *n == v4net)
                        .map(|i| { self.ipv4_cidrs.remove(i); true })
                        .unwrap_or(false);
                    let meta_removed = self.metadata.remove(&key).is_some();
                    cidr_removed || meta_removed
                }
            }
            IpNet::V6(v6net) => {
                let key = if v6net.prefix_len() == 128 {
                    v6net.addr().to_string()
                } else {
                    net.to_string()
                };

                let cidr_removed = if v6net.prefix_len() != 128 {
                    self.ipv6_cidrs.iter().position(|n| *n == v6net)
                        .map(|i| { self.ipv6_cidrs.remove(i); true })
                        .unwrap_or(false)
                } else {
                    false
                };

                let meta_removed = self.metadata.remove(&key).is_some();
                cidr_removed || meta_removed
            }
        };

        if removed {
            debug!(ban = %net, "Removed IP ban");
            self.save()?;
        }

        Ok(removed)
    }

    /// Remove a ban by IP address.
    #[allow(dead_code)] // Available for future admin commands
    pub fn remove_ban_ip(&mut self, ip: IpAddr) -> Result<bool, std::io::Error> {
        let net = match ip {
            IpAddr::V4(v4) => IpNet::V4(Ipv4Net::new(v4, 32).expect("prefix 32 is valid")),
            IpAddr::V6(v6) => IpNet::V6(Ipv6Net::new(v6, 128).expect("prefix 128 is valid")),
        };
        self.remove_ban(net)
    }

    /// Prune expired bans from the list.
    ///
    /// Called periodically by a background task.
    /// Returns the number of bans removed.
    #[allow(dead_code)] // Available for scheduled cleanup tasks
    pub fn prune_expired(&mut self) -> usize {
        let mut removed = 0;
        let mut expired_keys: Vec<String> = Vec::new();

        // Find expired metadata entries
        for (key, meta) in &self.metadata {
            if meta.is_expired() {
                expired_keys.push(key.clone());
            }
        }

        // Remove expired entries
        for key in expired_keys {
            // Try to parse as IP or CIDR to clean up bitmap/vecs
            if let Ok(ip) = key.parse::<IpAddr>() {
                if let IpAddr::V4(v4) = ip {
                    let ip_u32 = u32::from_be_bytes(v4.octets());
                    self.ipv4_bitmap.remove(ip_u32);
                }
            } else if let Ok(net) = key.parse::<Ipv4Net>() {
                self.ipv4_cidrs.retain(|n| *n != net);
            } else if let Ok(net) = key.parse::<Ipv6Net>() {
                self.ipv6_cidrs.retain(|n| *n != net);
            }

            self.metadata.remove(&key);
            removed += 1;
        }

        if removed > 0 {
            debug!(count = removed, "Pruned expired IP bans");
            if let Err(e) = self.save() {
                error!(error = %e, "Failed to persist after pruning expired bans");
            }
        }

        removed
    }

    /// Save the deny list to disk using MessagePack.
    ///
    /// Uses atomic write (temp file + rename) to prevent corruption.
    pub fn save(&self) -> Result<(), std::io::Error> {
        let state = PersistentState {
            ipv4_singles: self.ipv4_bitmap.iter().collect(),
            ipv4_cidrs: self.ipv4_cidrs.iter().map(|n| n.to_string()).collect(),
            ipv6_cidrs: self.ipv6_cidrs.iter().map(|n| n.to_string()).collect(),
            metadata: self.metadata.clone(),
        };

        // Write to temp file first
        let temp_path = self.persist_path.with_extension("msgpack.tmp");
        let file = File::create(&temp_path)?;
        let writer = BufWriter::new(file);

        rmp_serde::encode::write(&mut BufWriter::new(writer), &state)
            .map_err(std::io::Error::other)?;

        // Atomic rename
        fs::rename(&temp_path, &self.persist_path)?;

        debug!(path = %self.persist_path.display(), "IP deny list saved");
        Ok(())
    }

    /// Get the number of banned entries.
    #[allow(dead_code)] // Available for stats commands
    pub fn len(&self) -> usize {
        self.metadata.len()
    }

    /// Check if the deny list is empty.
    #[allow(dead_code)] // Available for stats commands
    pub fn is_empty(&self) -> bool {
        self.metadata.is_empty()
    }

    /// Get an iterator over all bans with their metadata.
    #[allow(dead_code)] // Available for STATS commands
    pub fn iter(&self) -> impl Iterator<Item = (&String, &BanMetadata)> {
        self.metadata.iter()
    }

    /// Get metadata for a specific ban.
    #[allow(dead_code)] // Available for admin queries
    pub fn get_metadata(&self, key: &str) -> Option<&BanMetadata> {
        self.metadata.get(key)
    }

    /// Reload the deny list from database (for REHASH command).
    ///
    /// This replaces the current in-memory state with fresh data from the database.
    /// Called when the REHASH command is issued by an operator.
    pub fn reload_from_database(
        &mut self,
        dlines: &[crate::db::Dline],
        zlines: &[crate::db::Zline],
    ) {
        // Clear current state - these operations are infallible for these types
        // RoaringBitmap::clear, Vec::clear, and HashMap::clear never panic
        let old_ipv4_count = self.ipv4_bitmap.len();
        let old_ipv4_cidr_count = self.ipv4_cidrs.len();
        let old_ipv6_count = self.ipv6_cidrs.len();
        
        self.ipv4_bitmap.clear();
        self.ipv4_cidrs.clear();
        self.ipv6_cidrs.clear();
        self.metadata.clear();

        // Reload from database
        let added = self.sync_from_database_bans(dlines, zlines);
        
        info!(
            cleared_ipv4 = old_ipv4_count,
            cleared_ipv4_cidrs = old_ipv4_cidr_count,
            cleared_ipv6 = old_ipv6_count,
            reloaded_ipv4 = self.ipv4_bitmap.len(),
            reloaded_ipv4_cidrs = self.ipv4_cidrs.len(),
            reloaded_ipv6 = self.ipv6_cidrs.len(),
            total = added,
            "IP deny list reloaded from database"
        );

        // Save to disk
        if let Err(e) = self.save() {
            error!(error = %e, "Failed to persist reloaded IP deny list");
        }
    }

    /// Synchronize with database D-lines and Z-lines at startup.
    ///
    /// Ensures any bans added via database admin tools (outside IRC handlers)
    /// are present in the IpDenyList for O(1) gateway checks.
    ///
    /// Called once at startup after loading from disk.
    /// Does NOT remove bans that are in IpDenyList but not in database
    /// (to preserve bans added via IRC commands).
    pub fn sync_from_database_bans(
        &mut self,
        dlines: &[crate::db::Dline],
        zlines: &[crate::db::Zline],
    ) -> usize {
        let mut added = 0;

        // Helper to calculate duration from expires_at timestamp
        let duration_from_expires = |expires_at: Option<i64>| -> Option<Duration> {
            expires_at.and_then(|exp| {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                if exp > now {
                    Some(Duration::from_secs((exp - now) as u64))
                } else {
                    None // Already expired
                }
            })
        };

        // Sync D-lines
        for dline in dlines {
            // Skip if already expired
            if let Some(expires) = dline.expires_at {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                if expires <= now {
                    continue;
                }
            }

            // Skip if already in IpDenyList
            if self.metadata.contains_key(&dline.mask) {
                continue;
            }

            // Try to parse as IpNet
            if let Ok(net) = dline.mask.parse::<IpNet>() {
                let reason = dline.reason.clone().unwrap_or_else(|| "D-lined".to_string());
                let duration = duration_from_expires(dline.expires_at);
                if self.add_ban(net, reason, duration, dline.set_by.clone()).is_ok() {
                    added += 1;
                }
            } else if let Ok(ip) = dline.mask.parse::<IpAddr>() {
                // Single IP without /prefix
                let net = match ip {
                    IpAddr::V4(v4) => IpNet::V4(Ipv4Net::new(v4, 32).expect("prefix 32 is valid")),
                    IpAddr::V6(v6) => IpNet::V6(Ipv6Net::new(v6, 128).expect("prefix 128 is valid")),
                };
                let reason = dline.reason.clone().unwrap_or_else(|| "D-lined".to_string());
                let duration = duration_from_expires(dline.expires_at);
                if self.add_ban(net, reason, duration, dline.set_by.clone()).is_ok() {
                    added += 1;
                }
            }
        }

        // Sync Z-lines
        for zline in zlines {
            // Skip if already expired
            if let Some(expires) = zline.expires_at {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                if expires <= now {
                    continue;
                }
            }

            // Skip if already in IpDenyList
            if self.metadata.contains_key(&zline.mask) {
                continue;
            }

            // Try to parse as IpNet
            if let Ok(net) = zline.mask.parse::<IpNet>() {
                let reason = zline.reason.clone().unwrap_or_else(|| "Z-lined".to_string());
                let duration = duration_from_expires(zline.expires_at);
                if self.add_ban(net, reason, duration, zline.set_by.clone()).is_ok() {
                    added += 1;
                }
            } else if let Ok(ip) = zline.mask.parse::<IpAddr>() {
                // Single IP without /prefix
                let net = match ip {
                    IpAddr::V4(v4) => IpNet::V4(Ipv4Net::new(v4, 32).expect("prefix 32 is valid")),
                    IpAddr::V6(v6) => IpNet::V6(Ipv6Net::new(v6, 128).expect("prefix 128 is valid")),
                };
                let reason = zline.reason.clone().unwrap_or_else(|| "Z-lined".to_string());
                let duration = duration_from_expires(zline.expires_at);
                if self.add_ban(net, reason, duration, zline.set_by.clone()).is_ok() {
                    added += 1;
                }
            }
        }

        if added > 0 {
            info!(
                dlines = dlines.len(),
                zlines = zlines.len(),
                added,
                "Synced database IP bans to IpDenyList"
            );
        }

        added
    }
}

impl Default for IpDenyList {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn test_add_and_check_ipv4_single() {
        let mut list = IpDenyList::with_path("/tmp/test_deny.msgpack");

        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        list.add_ban_ip(ip, "Test ban".to_string(), None, "test".to_string())
            .unwrap();

        // Should be banned
        assert!(list.check_ip(&ip).is_some());
        assert_eq!(list.check_ip(&ip).unwrap(), "Test ban");

        // Different IP should not be banned
        let other_ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 101));
        assert!(list.check_ip(&other_ip).is_none());
    }

    #[test]
    fn test_add_and_check_ipv4_cidr() {
        let mut list = IpDenyList::with_path("/tmp/test_deny_cidr.msgpack");

        let net: IpNet = "10.0.0.0/8".parse().unwrap();
        list.add_ban(net, "Network ban".to_string(), None, "test".to_string())
            .unwrap();

        // IPs in range should be banned
        let ip1 = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let ip2 = IpAddr::V4(Ipv4Addr::new(10, 255, 255, 255));
        assert!(list.check_ip(&ip1).is_some());
        assert!(list.check_ip(&ip2).is_some());

        // IP outside range should not be banned
        let outside = IpAddr::V4(Ipv4Addr::new(11, 0, 0, 1));
        assert!(list.check_ip(&outside).is_none());
    }

    #[test]
    fn test_add_and_check_ipv6() {
        let mut list = IpDenyList::with_path("/tmp/test_deny_ipv6.msgpack");

        let ip = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1));
        list.add_ban_ip(ip, "IPv6 ban".to_string(), None, "test".to_string())
            .unwrap();

        assert!(list.check_ip(&ip).is_some());

        // Different IPv6 should not be banned
        let other = IpAddr::V6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 2));
        assert!(list.check_ip(&other).is_none());
    }

    #[test]
    fn test_remove_ban() {
        let mut list = IpDenyList::with_path("/tmp/test_deny_remove.msgpack");

        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50));
        list.add_ban_ip(ip, "Temp ban".to_string(), None, "test".to_string())
            .unwrap();
        assert!(list.check_ip(&ip).is_some());

        list.remove_ban_ip(ip).unwrap();
        assert!(list.check_ip(&ip).is_none());
    }

    #[test]
    fn test_expiration() {
        let mut list = IpDenyList::with_path("/tmp/test_deny_expire.msgpack");

        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 200));

        // Create metadata directly with an already-expired timestamp
        let ip_str = ip.to_string();
        let expired_meta = BanMetadata {
            reason: "Expired ban".to_string(),
            expiry: Some(1), // Unix timestamp 1 = long ago (1970)
            added_by: "test".to_string(),
            added_at: 0,
        };

        // Add to bitmap
        if let IpAddr::V4(v4) = ip {
            let ip_u32 = u32::from_be_bytes(v4.octets());
            list.ipv4_bitmap.insert(ip_u32);
        }
        list.metadata.insert(ip_str, expired_meta);

        // Should not be considered banned (expired)
        assert!(list.check_ip(&ip).is_none());
    }

    #[test]
    fn test_persistence_roundtrip() {
        let path = "/tmp/test_deny_persist.msgpack";
        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));

        // Create and save
        {
            let mut list = IpDenyList::with_path(path);
            list.add_ban_ip(ip, "Persist test".to_string(), None, "test".to_string())
                .unwrap();
        }

        // Load and verify
        {
            let list = IpDenyList::load(path);
            assert!(list.check_ip(&ip).is_some());
            assert_eq!(list.check_ip(&ip).unwrap(), "Persist test");
        }

        // Cleanup
        let _ = fs::remove_file(path);
    }
}
