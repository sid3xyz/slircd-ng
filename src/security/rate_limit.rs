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
const NZ_1: NonZeroU32 = match NonZeroU32::new(1) {
    Some(v) => v,
    None => panic!("1 is non-zero"),
};
const NZ_2: NonZeroU32 = match NonZeroU32::new(2) {
    Some(v) => v,
    None => panic!("2 is non-zero"),
};
const NZ_3: NonZeroU32 = match NonZeroU32::new(3) {
    Some(v) => v,
    None => panic!("3 is non-zero"),
};
const NZ_5: NonZeroU32 = match NonZeroU32::new(5) {
    Some(v) => v,
    None => panic!("5 is non-zero"),
};

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
        self.last_access
            .store(current_timestamp(), Ordering::Relaxed);
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
            .map(|entry| {
                (
                    entry.key().clone(),
                    entry.value().last_access.load(Ordering::Relaxed),
                )
            })
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
            .map(|entry| {
                (
                    *entry.key(),
                    entry.value().last_access.load(Ordering::Relaxed),
                )
            })
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

// =============================================================================
// Server-to-Server Rate Limiting
// =============================================================================

/// Server ID type for S2S rate limiting.
type Sid = String;

/// S2S peer state with rate limiter and violation counter.
#[derive(Debug)]
pub struct S2SPeerState {
    /// Token bucket rate limiter for this peer.
    limiter: DirectRateLimiter,
    /// Number of rate limit violations since last reset.
    pub violations: std::sync::atomic::AtomicU32,
    /// Last access timestamp for debugging.
    last_access: AtomicU64,
}

impl S2SPeerState {
    fn new(rate: NonZeroU32, burst: NonZeroU32) -> Self {
        Self {
            limiter: GovRateLimiter::direct(Quota::per_second(rate).allow_burst(burst)),
            violations: std::sync::atomic::AtomicU32::new(0),
            last_access: AtomicU64::new(current_timestamp()),
        }
    }

    /// Check if a command is allowed. Returns (allowed, should_disconnect).
    fn check(&self, disconnect_threshold: u32) -> (bool, bool) {
        self.last_access
            .store(current_timestamp(), Ordering::Relaxed);

        if self.limiter.check().is_ok() {
            // Reset violations on successful check (sliding window behavior)
            self.violations.store(0, Ordering::Relaxed);
            (true, false)
        } else {
            // Increment violation counter
            let violations = self.violations.fetch_add(1, Ordering::Relaxed) + 1;
            let should_disconnect = violations >= disconnect_threshold;
            (false, should_disconnect)
        }
    }

    /// Get current violation count (test-only).
    #[cfg(test)]
    pub fn violation_count(&self) -> u32 {
        self.violations.load(Ordering::Relaxed)
    }
}

/// Rate limiter for server-to-server links.
///
/// Prevents a compromised or misbehaving peer from flooding the local server.
/// Uses a token bucket algorithm with configurable rates and disconnect threshold.
#[derive(Debug)]
pub struct S2SRateLimiter {
    /// Per-peer rate limiters, keyed by server ID (SID).
    peers: DashMap<Sid, S2SPeerState>,
    /// Commands allowed per second per peer.
    rate: NonZeroU32,
    /// Burst allowance per peer.
    burst: NonZeroU32,
    /// Number of violations before disconnecting.
    disconnect_threshold: u32,
}

impl S2SRateLimiter {
    /// Create a new S2S rate limiter from configuration.
    pub fn new(config: &RateLimitConfig) -> Self {
        Self {
            peers: DashMap::new(),
            rate: NonZeroU32::new(config.s2s_command_rate_per_second).unwrap_or(
                // SAFETY: 100 is non-zero
                NonZeroU32::new(100).unwrap(),
            ),
            burst: NonZeroU32::new(config.s2s_burst_per_peer).unwrap_or(
                // SAFETY: 500 is non-zero
                NonZeroU32::new(500).unwrap(),
            ),
            disconnect_threshold: config.s2s_disconnect_threshold,
        }
    }

    /// Check if a command from a peer is allowed.
    ///
    /// Returns `Ok(())` if allowed, `Err(S2SRateLimitResult)` if limited or should disconnect.
    pub fn check_command(&self, sid: &str) -> S2SRateLimitResult {
        let entry = self
            .peers
            .entry(sid.to_string())
            .or_insert_with(|| S2SPeerState::new(self.rate, self.burst));

        let (allowed, should_disconnect) = entry.check(self.disconnect_threshold);

        if should_disconnect {
            S2SRateLimitResult::Disconnect {
                violations: entry.violations.load(Ordering::Relaxed),
            }
        } else if allowed {
            S2SRateLimitResult::Allowed
        } else {
            S2SRateLimitResult::Limited {
                violations: entry.violations.load(Ordering::Relaxed),
            }
        }
    }

    /// Remove a peer's rate limit state (call on disconnect).
    pub fn remove_peer(&self, sid: &str) {
        self.peers.remove(sid);
    }

    /// Get the current violation count for a peer (test-only).
    #[cfg(test)]
    pub fn violation_count(&self, sid: &str) -> u32 {
        self.peers
            .get(sid)
            .map(|p| p.violation_count())
            .unwrap_or(0)
    }
}

impl Default for S2SRateLimiter {
    fn default() -> Self {
        Self::new(&RateLimitConfig::default())
    }
}

/// Result of an S2S rate limit check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum S2SRateLimitResult {
    /// Command is allowed.
    Allowed,
    /// Command is rate limited but peer should not be disconnected yet.
    Limited {
        /// Current violation count.
        violations: u32,
    },
    /// Too many violations - peer should be disconnected.
    Disconnect {
        /// Final violation count.
        violations: u32,
    },
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
            s2s_command_rate_per_second: 100,
            s2s_burst_per_peer: 500,
            s2s_disconnect_threshold: 10,
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

    // === S2S Rate Limiting Tests ===

    fn s2s_test_config() -> RateLimitConfig {
        RateLimitConfig {
            s2s_command_rate_per_second: 5,
            s2s_burst_per_peer: 10,
            s2s_disconnect_threshold: 3,
            ..test_config()
        }
    }

    #[test]
    fn test_s2s_rate_limit_burst_allowed() {
        let limiter = S2SRateLimiter::new(&s2s_test_config());
        let sid = "00A";

        // First 10 commands should be allowed (burst of 10)
        for i in 0..10 {
            let result = limiter.check_command(sid);
            assert_eq!(
                result,
                S2SRateLimitResult::Allowed,
                "Command {} should be allowed",
                i
            );
        }
    }

    #[test]
    fn test_s2s_rate_limit_exceeded() {
        let limiter = S2SRateLimiter::new(&s2s_test_config());
        let sid = "00B";

        // Exhaust burst
        for _ in 0..10 {
            limiter.check_command(sid);
        }

        // Next commands should be limited (but not disconnected yet)
        let result = limiter.check_command(sid);
        assert!(matches!(
            result,
            S2SRateLimitResult::Limited { violations: 1 }
        ));

        let result = limiter.check_command(sid);
        assert!(matches!(
            result,
            S2SRateLimitResult::Limited { violations: 2 }
        ));
    }

    #[test]
    fn test_s2s_disconnect_threshold() {
        let limiter = S2SRateLimiter::new(&s2s_test_config());
        let sid = "00C";

        // Exhaust burst
        for _ in 0..10 {
            limiter.check_command(sid);
        }

        // Violate 3 times (threshold is 3)
        limiter.check_command(sid); // violation 1
        limiter.check_command(sid); // violation 2
        let result = limiter.check_command(sid); // violation 3 - should disconnect

        assert!(
            matches!(result, S2SRateLimitResult::Disconnect { violations: 3 }),
            "Expected Disconnect after 3 violations, got {:?}",
            result
        );
    }

    #[test]
    fn test_s2s_peer_removal() {
        let limiter = S2SRateLimiter::new(&s2s_test_config());
        let sid = "00D";

        // Exhaust burst and create violations
        for _ in 0..12 {
            limiter.check_command(sid);
        }
        assert!(limiter.violation_count(sid) > 0);

        // Remove peer
        limiter.remove_peer(sid);

        // Peer should have fresh state
        assert_eq!(limiter.violation_count(sid), 0);
        assert_eq!(limiter.check_command(sid), S2SRateLimitResult::Allowed);
    }

    #[test]
    fn test_s2s_peers_independent() {
        let limiter = S2SRateLimiter::new(&s2s_test_config());
        let sid1 = "00E";
        let sid2 = "00F";

        // Exhaust sid1's burst
        for _ in 0..10 {
            limiter.check_command(sid1);
        }

        // sid1 should be limited
        assert!(matches!(
            limiter.check_command(sid1),
            S2SRateLimitResult::Limited { .. }
        ));

        // sid2 should still be allowed
        assert_eq!(limiter.check_command(sid2), S2SRateLimitResult::Allowed);
    }
}
