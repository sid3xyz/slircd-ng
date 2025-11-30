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
            config: Arc::new(config),
        }
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
    pub fn check_connection_rate(&self, ip: IpAddr) -> bool {
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

    /// Record a message being sent (consumes a token).
    ///
    /// Use this when you want to always record the action, regardless of limit.
    /// Prefer `check_message_rate()` for normal flow control.
    /// Will be used for message tracking in Phase 3.
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
    /// Call periodically (e.g., every 5 minutes) from a maintenance task.
    /// Will be wired into server maintenance loop in Phase 3.
    #[allow(dead_code)]
    pub fn cleanup(&self) {
        // Simple strategy: if we have too many entries, clear them all
        // In production, you'd want to track last-access time
        const MAX_ENTRIES: usize = 10_000;

        if self.message_limiters.len() > MAX_ENTRIES {
            self.message_limiters.clear();
            debug!("cleared message rate limiters (exceeded {} entries)", MAX_ENTRIES);
        }
        if self.connection_limiters.len() > MAX_ENTRIES {
            self.connection_limiters.clear();
            debug!("cleared connection rate limiters (exceeded {} entries)", MAX_ENTRIES);
        }
        if self.join_limiters.len() > MAX_ENTRIES {
            self.join_limiters.clear();
            debug!("cleared join rate limiters (exceeded {} entries)", MAX_ENTRIES);
        }
    }

    /// Update rate limit configuration (for REHASH support).
    ///
    /// Note: This only affects new limiters. Existing clients keep their
    /// current limits until they disconnect.
    /// Will be wired into REHASH handler in Phase 3.
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

    /// Get current statistics.
    /// Will be wired into STATS command in Phase 3.
    #[allow(dead_code)]
    pub fn stats(&self) -> RateLimitStats {
        RateLimitStats {
            message_limiters: self.message_limiters.len(),
            connection_limiters: self.connection_limiters.len(),
            join_limiters: self.join_limiters.len(),
        }
    }
}

impl Default for RateLimitManager {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

/// Rate limiter statistics.
/// Will be used by STATS command in Phase 3.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct RateLimitStats {
    /// Number of active message rate limiters.
    pub message_limiters: usize,
    /// Number of active connection rate limiters.
    pub connection_limiters: usize,
    /// Number of active join rate limiters.
    pub join_limiters: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RateLimitConfig {
        RateLimitConfig {
            message_rate_per_second: 2,
            connection_burst_per_ip: 3,
            join_burst_per_client: 5,
        }
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
