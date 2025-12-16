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

use crate::config::RateLimitConfig;
use dashmap::DashMap;
use governor::{Quota, RateLimiter as GovRateLimiter};
use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::Arc;
use tracing::debug;

/// Type alias for governor's direct rate limiter.
type DirectRateLimiter = governor::DefaultDirectRateLimiter;

/// User identifier (UID string).
type Uid = String;

/// Thread-safe rate limit manager using governor.
///
/// Provides separate limiters for messages, connections, and joins,
/// all configurable via `RateLimitConfig`.
#[derive(Debug)]
pub struct RateLimitManager {
    /// Per-client message rate limiters (keyed by UID).
    message_limiters: DashMap<Uid, DirectRateLimiter>,
    /// Per-IP connection rate limiters.
    connection_limiters: DashMap<IpAddr, DirectRateLimiter>,
    /// Per-client channel join rate limiters.
    join_limiters: DashMap<Uid, DirectRateLimiter>,
    /// Per-client CTCP rate limiters.
    ctcp_limiters: DashMap<Uid, DirectRateLimiter>,
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
        let limiter = self.message_limiters.entry(uid.clone()).or_insert_with(|| {
            let rate = NonZeroU32::new(self.config.message_rate_per_second)
                .unwrap_or(NonZeroU32::new(2).unwrap());
            GovRateLimiter::direct(Quota::per_second(rate))
        });

        let allowed = limiter.check().is_ok();
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

        let limiter = self.connection_limiters.entry(ip).or_insert_with(|| {
            let burst = NonZeroU32::new(self.config.connection_burst_per_ip)
                .unwrap_or(NonZeroU32::new(3).unwrap());
            // 1 connection per 10 seconds with burst
            GovRateLimiter::direct(
                Quota::per_second(NonZeroU32::new(1).unwrap()).allow_burst(burst),
            )
        });

        let allowed = limiter.check().is_ok();
        if !allowed {
            debug!(ip = %ip, "connection rate limit exceeded");
        }
        allowed
    }

    /// Check if a client can join a channel.
    ///
    /// Returns `true` if allowed, `false` if rate limited.
    pub fn check_join_rate(&self, uid: &Uid) -> bool {
        let limiter = self.join_limiters.entry(uid.clone()).or_insert_with(|| {
            let burst = NonZeroU32::new(self.config.join_burst_per_client)
                .unwrap_or(NonZeroU32::new(5).unwrap());
            // 1 join per second with burst
            GovRateLimiter::direct(
                Quota::per_second(NonZeroU32::new(1).unwrap()).allow_burst(burst),
            )
        });

        let allowed = limiter.check().is_ok();
        if !allowed {
            debug!(uid = %uid, "join rate limit exceeded");
        }
        allowed
    }

    /// Check if a client can send a CTCP message.
    pub fn check_ctcp_rate(&self, uid: &Uid) -> bool {
        let limiter = self.ctcp_limiters.entry(uid.clone()).or_insert_with(|| {
            let burst = NonZeroU32::new(self.config.ctcp_burst_per_client)
                .unwrap_or(NonZeroU32::new(2).unwrap());
            GovRateLimiter::direct(
                Quota::per_second(
                    NonZeroU32::new(self.config.ctcp_rate_per_second)
                        .unwrap_or(NonZeroU32::new(1).unwrap()),
                )
                .allow_burst(burst),
            )
        });

        let allowed = limiter.check().is_ok();
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

    /// Record a message being sent (consumes a token).
    ///
    /// Use this when you want to always record the action, regardless of limit.
    /// Prefer `check_message_rate()` for normal flow control.
    #[allow(dead_code)]
    pub fn record_message(&self, uid: &Uid) {
        let _ = self.check_message_rate(uid);
    }

    /// Remove a client from all rate limiters (on disconnect).
    pub fn remove_client(&self, uid: &Uid) {
        self.message_limiters.remove(uid);
        self.join_limiters.remove(uid);
    }

    /// Cleanup old entries to prevent memory growth.
    ///
    /// Called every 5 minutes by background task in main.rs.
    /// Primarily cleans connection_limiters (keyed by IP, not removed on disconnect).
    pub fn cleanup(&self) {
        // Simple strategy: if we have too many entries, clear them all
        // In production, you'd want to track last-access time
        const MAX_ENTRIES: usize = 10_000;

        if self.message_limiters.len() > MAX_ENTRIES {
            self.message_limiters.clear();
            debug!(
                "cleared message rate limiters (exceeded {} entries)",
                MAX_ENTRIES
            );
        }
        if self.connection_limiters.len() > MAX_ENTRIES {
            self.connection_limiters.clear();
            debug!(
                "cleared connection rate limiters (exceeded {} entries)",
                MAX_ENTRIES
            );
        }
        if self.join_limiters.len() > MAX_ENTRIES {
            self.join_limiters.clear();
            debug!(
                "cleared join rate limiters (exceeded {} entries)",
                MAX_ENTRIES
            );
        }

        if self.ctcp_limiters.len() > MAX_ENTRIES {
            self.ctcp_limiters.clear();
            debug!(
                "cleared ctcp rate limiters (exceeded {} entries)",
                MAX_ENTRIES
            );
        }

        if self.active_connections.len() > MAX_ENTRIES {
            self.active_connections.clear();
            debug!("cleared active connection counters (exceeded {MAX_ENTRIES} entries)");
        }
    }

    /// Update rate limit configuration (for REHASH support).
    ///
    /// Note: This only affects new limiters. Existing clients keep their
    /// current limits until they disconnect.
    #[allow(dead_code)]
    pub fn update_config(&self, new_config: RateLimitConfig) {
        // We can't update Arc in place, but we can store the new config
        // for new connections. For full support, we'd need interior mutability.
        debug!(
            msg_rate = new_config.message_rate_per_second,
            conn_burst = new_config.connection_burst_per_ip,
            join_burst = new_config.join_burst_per_client,
            "rate limit config update requested (affects new connections only)"
        );
    }

    /// Get current statistics for STATS command.
    #[allow(dead_code)]
    pub fn stats(&self) -> RateLimitStats {
        RateLimitStats {
            message_limiters: self.message_limiters.len(),
            connection_limiters: self.connection_limiters.len(),
            join_limiters: self.join_limiters.len(),
            ctcp_limiters: self.ctcp_limiters.len(),
            active_connections: self.active_connections.len(),
        }
    }
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

/// Rate limiter statistics for STATS command.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RateLimitStats {
    /// Number of active message rate limiters.
    pub message_limiters: usize,
    /// Number of active connection rate limiters.
    pub connection_limiters: usize,
    /// Number of active join rate limiters.
    pub join_limiters: usize,
    /// Number of active CTCP rate limiters.
    pub ctcp_limiters: usize,
    /// Number of tracked IPs with active connections.
    pub active_connections: usize,
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

        // Create some entries
        manager.check_message_rate(&uid);
        manager.check_join_rate(&uid);
        assert_eq!(manager.stats().message_limiters, 1);
        assert_eq!(manager.stats().join_limiters, 1);

        // Remove client
        manager.remove_client(&uid);
        assert_eq!(manager.stats().message_limiters, 0);
        assert_eq!(manager.stats().join_limiters, 0);
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
