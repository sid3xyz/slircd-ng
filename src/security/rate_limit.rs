//! Rate limiting for flood protection.
//!
//! Provides governor-based rate limiting for:
//! - Message rate per client
//! - Connection rate per IP
//! - Channel join rate per client
//!
//! # Architecture
//!
//! Uses the `governor` crate's token bucket algorithm with configurable
//! rates and bursts. Each limiter type has its own storage to prevent
//! interference.
//!
//! # LRU Eviction
//!
//! When entries exceed MAX_ENTRIES, uses LRU (Least Recently Used) eviction
//! to remove the oldest entries rather than clearing all entries. This
//! preserves rate limiting state for active clients.

use crate::config::RateLimitConfig;
use dashmap::DashMap;
use governor::{Quota, RateLimiter as GovRateLimiter};
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

// Safe NonZeroU32 constants - these are compile-time verified non-zero values
const NZ_1: NonZeroU32 = match NonZeroU32::new(1) { Some(v) => v, None => panic!("1 is non-zero") };
const NZ_2: NonZeroU32 = match NonZeroU32::new(2) { Some(v) => v, None => panic!("2 is non-zero") };
const NZ_3: NonZeroU32 = match NonZeroU32::new(3) { Some(v) => v, None => panic!("3 is non-zero") };
const NZ_5: NonZeroU32 = match NonZeroU32::new(5) { Some(v) => v, None => panic!("5 is non-zero") };

/// Type alias for governor's direct rate limiter.
type DirectRateLimiter = governor::DefaultDirectRateLimiter;

/// User identifier (UID string).
type Uid = String;

/// Maximum number of entries before LRU eviction triggers.
const MAX_ENTRIES: usize = 10_000;

/// Number of entries to evict when LRU cleanup runs.
/// Evicting 20% at a time avoids frequent cleanup cycles.
const EVICTION_COUNT: usize = 2_000;

/// Rate limiter entry with last-access timestamp for LRU eviction.
#[derive(Debug)]
struct TimedLimiter {
    limiter: DirectRateLimiter,
    last_access: AtomicU64, // Unix timestamp in seconds
}

impl TimedLimiter {
    fn new(limiter: DirectRateLimiter) -> Self {
        Self {
            limiter,
            last_access: AtomicU64::new(current_timestamp()),
        }
    }

    fn touch(&self) {
        self.last_access.store(current_timestamp(), Ordering::Relaxed);
    }

    fn check(&self) -> bool {
        self.touch();
        self.limiter.check().is_ok()
    }
}

/// Get current Unix timestamp in seconds.
fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Thread-safe rate limit manager using governor.
///
/// Provides separate limiters for messages, connections, and joins,
/// all configurable via `RateLimitConfig`.
#[derive(Debug)]
pub struct RateLimitManager {
    /// Per-client message rate limiters (keyed by UID).
    message_limiters: DashMap<Uid, TimedLimiter>,
    /// Per-IP connection rate limiters.
    connection_limiters: DashMap<IpAddr, TimedLimiter>,
    /// Per-client channel join rate limiters.
    join_limiters: DashMap<Uid, TimedLimiter>,
    /// Per-client CTCP rate limiters.
    ctcp_limiters: DashMap<Uid, TimedLimiter>,
    /// Active connection counters per IP.
    active_connections: DashMap<IpAddr, u32>,
    /// Configuration values.
    config: Arc<RateLimitConfig>,
}

impl RateLimitManager {
    /// Create a new rate limit manager with the given configuration.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            message_limiters: DashMap::new(),
            connection_limiters: DashMap::new(),
            join_limiters: DashMap::new(),
            ctcp_limiters: DashMap::new(),
            active_connections: DashMap::new(),
            config: Arc::new(config),
        }
    }

    /// Check if an IP address is exempt from rate limiting.
    ///
    /// Exempt IPs bypass all rate limits and connection limits.
    /// Use sparingly for trusted operators/bots only.
    pub fn is_exempt(&self, ip: IpAddr) -> bool {
        let ip_str = ip.to_string();
        self.config.exempt_ips.contains(&ip_str)
    }

    /// Check if a client can send a message.
    ///
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check_message_rate(&self, uid: &Uid) -> bool {
        let entry = self.message_limiters.entry(uid.clone()).or_insert_with(|| {
            let rate = NonZeroU32::new(self.config.message_rate_per_second).unwrap_or(NZ_2);
            TimedLimiter::new(GovRateLimiter::direct(Quota::per_second(rate)))
        });

        let allowed = entry.check();
        if !allowed {
            debug!(uid = %uid, "message rate limit exceeded");
        }
        allowed
    }

    /// Check if an IP can make a new connection.
    ///
    /// Returns `true` if allowed, `false` if rate limited.
    /// Exempt IPs always return `true`.
    pub fn check_connection_rate(&self, ip: IpAddr) -> bool {
        // Exempt IPs bypass rate limiting
        if self.is_exempt(ip) {
            return true;
        }

        let entry = self.connection_limiters.entry(ip).or_insert_with(|| {
            let burst = NonZeroU32::new(self.config.connection_burst_per_ip).unwrap_or(NZ_3);
            // 1 connection per 10 seconds with burst
            TimedLimiter::new(GovRateLimiter::direct(
                Quota::per_second(NZ_1).allow_burst(burst),
            ))
        });

        let allowed = entry.check();
        if !allowed {
            debug!(ip = %ip, "connection rate limit exceeded");
        }
        allowed
    }

    /// Check if a client can join a channel.
    ///
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check_join_rate(&self, uid: &Uid) -> bool {
        let entry = self.join_limiters.entry(uid.clone()).or_insert_with(|| {
            let burst = NonZeroU32::new(self.config.join_burst_per_client).unwrap_or(NZ_5);
            // 1 join per second with burst
            TimedLimiter::new(GovRateLimiter::direct(
                Quota::per_second(NZ_1).allow_burst(burst),
            ))
        });

        let allowed = entry.check();
        if !allowed {
            debug!(uid = %uid, "join rate limit exceeded");
        }
        allowed
    }

    /// Check if a client can send a CTCP message.
    pub fn check_ctcp_rate(&self, uid: &Uid) -> bool {
        let entry = self.ctcp_limiters.entry(uid.clone()).or_insert_with(|| {
            let burst = NonZeroU32::new(self.config.ctcp_burst_per_client).unwrap_or(NZ_2);
            TimedLimiter::new(GovRateLimiter::direct(
                Quota::per_second(
                    NonZeroU32::new(self.config.ctcp_rate_per_second).unwrap_or(NZ_1),
                )
                .allow_burst(burst),
            ))
        });

        let allowed = entry.check();
        if !allowed {
            debug!(uid = %uid, "ctcp rate limit exceeded");
        }
        allowed
    }

    /// Record that a connection has started for an IP.
    /// Returns `true` if allowed, `false` if max connections per IP exceeded.
    /// Exempt IPs always return `true` and are not tracked.
    pub fn on_connection_start(&self, ip: IpAddr) -> bool {
        // Exempt IPs bypass connection limits entirely
        if self.is_exempt(ip) {
            return true;
        }

        let mut allowed = true;
        self.active_connections
            .entry(ip)
            .and_modify(|count| {
                if *count >= self.config.max_connections_per_ip {
                    allowed = false;
                } else {
                    *count += 1;
                }
            })
            .or_insert(1);

        if !allowed {
            debug!(ip = %ip, limit = self.config.max_connections_per_ip, "max connections per IP exceeded");
        }
        allowed
    }

    /// Record that a connection has ended for an IP.
    /// Does nothing for exempt IPs (they aren't tracked).
    pub fn on_connection_end(&self, ip: IpAddr) {
        // Exempt IPs aren't tracked, so nothing to clean up
        if self.is_exempt(ip) {
            return;
        }

        // Use entry API for atomic check-and-modify/remove
        // We need to avoid holding a reference while calling remove()
        let should_remove = {
            if let Some(mut entry) = self.active_connections.get_mut(&ip) {
                if *entry > 1 {
                    *entry -= 1;
                    false // Don't remove, we decremented
                } else {
                    true // Will remove after dropping the entry
                }
            } else {
                false // Entry doesn't exist
            }
        }; // entry is dropped here, releasing the shard lock

        if should_remove {
            self.active_connections.remove(&ip);
        }
    }

    /// Remove a client from all rate limiters (on disconnect).
    pub fn remove_client(&self, uid: &Uid) {
        self.message_limiters.remove(uid);
        self.join_limiters.remove(uid);
        self.ctcp_limiters.remove(uid);
    }

    /// Cleanup old entries to prevent memory growth using LRU eviction.
    ///
    /// Called every 5 minutes by background task in main.rs.
    /// Uses Least Recently Used (LRU) eviction to remove the oldest entries
    /// rather than clearing all entries. This preserves rate limiting state
    /// for active clients while bounding memory usage.
    pub fn cleanup(&self) {
        self.evict_lru_uid_entries(&self.message_limiters, "message");
        self.evict_lru_ip_entries(&self.connection_limiters, "connection");
        self.evict_lru_uid_entries(&self.join_limiters, "join");
        self.evict_lru_uid_entries(&self.ctcp_limiters, "ctcp");

        // Active connections use simple count, not limiters - just log if large
        if self.active_connections.len() > MAX_ENTRIES {
            debug!(
                count = self.active_connections.len(),
                "active_connections exceeds MAX_ENTRIES but cannot evict (active state)"
            );
        }
    }

    /// Evict least recently used entries from a UID-keyed DashMap.
    fn evict_lru_uid_entries(&self, map: &DashMap<Uid, TimedLimiter>, name: &str) {
        if map.len() <= MAX_ENTRIES {
            return;
        }

        // Collect entries with their last access times
        // We can't hold refs while removing, so collect keys and timestamps first
        let mut entries: Vec<(Uid, u64)> = map
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().last_access.load(Ordering::Relaxed)))
            .collect();

        // Sort by last access time (oldest first)
        entries.sort_by_key(|(_k, ts)| *ts);

        // Remove the oldest EVICTION_COUNT entries
        let to_evict = entries.into_iter().take(EVICTION_COUNT);
        let mut evicted = 0;
        for (key, _) in to_evict {
            map.remove(&key);
            evicted += 1;
        }

        debug!(
            limiter_type = name,
            evicted = evicted,
            remaining = map.len(),
            "LRU eviction completed"
        );
    }

    /// Evict least recently used entries from an IP-keyed DashMap.
    fn evict_lru_ip_entries(&self, map: &DashMap<IpAddr, TimedLimiter>, name: &str) {
        if map.len() <= MAX_ENTRIES {
            return;
        }

        // Collect entries with their last access times
        let mut entries: Vec<(IpAddr, u64)> = map
            .iter()
            .map(|entry| (*entry.key(), entry.value().last_access.load(Ordering::Relaxed)))
            .collect();

        // Sort by last access time (oldest first)
        entries.sort_by_key(|(_k, ts)| *ts);

        // Remove the oldest EVICTION_COUNT entries
        let to_evict = entries.into_iter().take(EVICTION_COUNT);
        let mut evicted = 0;
        for (key, _) in to_evict {
            map.remove(&key);
            evicted += 1;
        }

        debug!(
            limiter_type = name,
            evicted = evicted,
            remaining = map.len(),
            "LRU eviction completed"
        );
    }
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RateLimitConfig {
        RateLimitConfig {
            message_rate_per_second: 2,
            connection_burst_per_ip: 3,
            join_burst_per_client: 5,
            ctcp_rate_per_second: 1,
            ctcp_burst_per_client: 2,
            max_connections_per_ip: 3,
            exempt_ips: Vec::new(),
        }
    }

    #[test]
    fn test_max_connections_per_ip() {
        let manager = RateLimitManager::new(test_config());
        let ip: IpAddr = "192.168.1.2".parse().unwrap();

        // First 3 connections allowed (limit is 3)
        assert!(manager.on_connection_start(ip));
        assert!(manager.on_connection_start(ip));
        assert!(manager.on_connection_start(ip));

        // Fourth should be rejected
        assert!(!manager.on_connection_start(ip));

        // One disconnects
        manager.on_connection_end(ip);

        // Should be allowed again
        assert!(manager.on_connection_start(ip));
    }

    #[test]
    fn test_message_rate_limiting() {
        let manager = RateLimitManager::new(test_config());
        let uid = "000AAAAAA".to_string();

        // First 2 messages should be allowed (rate is 2/sec)
        assert!(manager.check_message_rate(&uid));
        assert!(manager.check_message_rate(&uid));

        // Third should be rate limited (no burst configured for messages)
        assert!(!manager.check_message_rate(&uid));
    }

    #[test]
    fn test_connection_rate_limiting() {
        let manager = RateLimitManager::new(test_config());
        let ip: IpAddr = "192.168.1.1".parse().unwrap();

        // First 3 connections should be allowed (burst of 3)
        assert!(manager.check_connection_rate(ip));
        assert!(manager.check_connection_rate(ip));
        assert!(manager.check_connection_rate(ip));

        // Fourth should be rate limited
        assert!(!manager.check_connection_rate(ip));
    }

    #[test]
    fn test_ctcp_rate_limiting() {
        let manager = RateLimitManager::new(test_config());
        let uid = "000AAAAAA".to_string();

        // Burst of 2 allowed
        assert!(manager.check_ctcp_rate(&uid));
        assert!(manager.check_ctcp_rate(&uid));
        // Third should be rate limited
        assert!(!manager.check_ctcp_rate(&uid));
    }

    #[test]
    fn test_join_rate_limiting() {
        let manager = RateLimitManager::new(test_config());
        let uid = "000AAAAAB".to_string();

        // First 5 joins should be allowed (burst of 5)
        for _ in 0..5 {
            assert!(manager.check_join_rate(&uid));
        }

        // Sixth should be rate limited
        assert!(!manager.check_join_rate(&uid));
    }

    #[test]
    fn test_client_removal() {
        let manager = RateLimitManager::new(test_config());
        let uid = "000AAAAAC".to_string();

        // Create some entries and consume tokens
        manager.check_message_rate(&uid); // 1st
        manager.check_message_rate(&uid); // 2nd (limit is 2, should be at limit)
        manager.check_join_rate(&uid);

        // Remove client
        manager.remove_client(&uid);

        // After removal, the client should get fresh rate limits
        // (should be able to send messages again since the limiter was removed)
        assert!(manager.check_message_rate(&uid)); // Should succeed - new limiter
    }

    #[test]
    fn test_different_clients_independent() {
        let manager = RateLimitManager::new(test_config());
        let uid1 = "000AAAAAD".to_string();
        let uid2 = "000AAAAAE".to_string();

        // Exhaust uid1's limit
        manager.check_message_rate(&uid1);
        manager.check_message_rate(&uid1);
        assert!(!manager.check_message_rate(&uid1));

        // uid2 should still be able to send
        assert!(manager.check_message_rate(&uid2));
    }
}
