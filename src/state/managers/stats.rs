//! Runtime statistics manager.
//!
//! Provides atomic counters for accurate real-time server metrics.
//! Used by `LUSERS` and `STATS` commands.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// Manages server runtime statistics with atomic counters.
///
/// All counters are thread-safe and use relaxed ordering for performance.
/// Exact consistency is not required for statistics.
#[derive(Debug)]
pub struct StatsManager {
    /// Local users connected to this server.
    local_users: AtomicUsize,
    /// Global users across the network (includes local).
    global_users: AtomicUsize,
    /// Users with +i (invisible) mode.
    invisible_users: AtomicUsize,
    /// Local operators on this server.
    local_opers: AtomicUsize,
    /// Global operators across the network.
    global_opers: AtomicUsize,
    /// Active channels.
    channels: AtomicUsize,
    /// Total connections since startup.
    connections_total: AtomicUsize,
    /// Peak concurrent connections.
    peak_connections: AtomicUsize,
    /// Peak global users.
    peak_global_users: AtomicUsize,
    /// Unregistered connections.
    unregistered_connections: AtomicUsize,
    /// Server startup time.
    started_at: Instant,
}

impl StatsManager {
    /// Create a new stats manager.
    pub fn new() -> Self {
        Self {
            local_users: AtomicUsize::new(0),
            global_users: AtomicUsize::new(0),
            invisible_users: AtomicUsize::new(0),
            local_opers: AtomicUsize::new(0),
            global_opers: AtomicUsize::new(0),
            channels: AtomicUsize::new(0),
            connections_total: AtomicUsize::new(0),
            peak_connections: AtomicUsize::new(0),
            peak_global_users: AtomicUsize::new(0),
            unregistered_connections: AtomicUsize::new(0),
            started_at: Instant::now(),
        }
    }

    // === User Counters ===

    /// Increment local user count. Returns new count.
    pub fn user_connected(&self) -> usize {
        let new_local = self.local_users.fetch_add(1, Ordering::Relaxed) + 1;
        let new_global = self.global_users.fetch_add(1, Ordering::Relaxed) + 1;
        self.connections_total.fetch_add(1, Ordering::Relaxed);

        // Update peak if needed
        let mut peak = self.peak_connections.load(Ordering::Relaxed);
        while new_local > peak {
            match self.peak_connections.compare_exchange_weak(
                peak,
                new_local,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => peak = current,
            }
        }

        // Update global peak
        let mut peak_global = self.peak_global_users.load(Ordering::Relaxed);
        while new_global > peak_global {
            match self.peak_global_users.compare_exchange_weak(
                peak_global,
                new_global,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => peak_global = current,
            }
        }

        new_global
    }

    /// Decrement local user count. Returns new global count.
    pub fn user_disconnected(&self) -> usize {
        self.local_users.fetch_sub(1, Ordering::Relaxed);
        self.global_users
            .fetch_sub(1, Ordering::Relaxed)
            .saturating_sub(1)
    }

    /// Increment a remote user count (global only).
    pub fn remote_user_connected(&self) {
        let new_global = self.global_users.fetch_add(1, Ordering::Relaxed) + 1;

        // Update global peak
        let mut peak_global = self.peak_global_users.load(Ordering::Relaxed);
        while new_global > peak_global {
            match self.peak_global_users.compare_exchange_weak(
                peak_global,
                new_global,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(current) => peak_global = current,
            }
        }
    }

    /// Decrement a remote user count (global only).
    pub fn remote_user_disconnected(&self) {
        self.global_users.fetch_sub(1, Ordering::Relaxed);
    }

    /// Increment invisible user count.
    pub fn user_set_invisible(&self) {
        self.invisible_users.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement invisible user count.
    pub fn user_unset_invisible(&self) {
        self.invisible_users.fetch_sub(1, Ordering::Relaxed);
    }

    /// Increment local operator count.
    pub fn user_opered(&self) {
        self.local_opers.fetch_add(1, Ordering::Relaxed);
        self.global_opers.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement local operator count.
    pub fn user_deopered(&self) {
        self.local_opers.fetch_sub(1, Ordering::Relaxed);
        self.global_opers.fetch_sub(1, Ordering::Relaxed);
    }

    /// Increment a remote operator count (global only).
    pub fn remote_user_opered(&self) {
        self.global_opers.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement a remote operator count (global only).
    pub fn remote_user_deopered(&self) {
        self.global_opers.fetch_sub(1, Ordering::Relaxed);
    }

    /// Increment unregistered connection count.
    pub fn increment_unregistered(&self) {
        self.unregistered_connections
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement unregistered connection count.
    pub fn decrement_unregistered(&self) {
        self.unregistered_connections
            .fetch_sub(1, Ordering::Relaxed);
    }

    // === Channel Counters ===

    /// Increment channel count.
    pub fn channel_created(&self) {
        self.channels.fetch_add(1, Ordering::Relaxed);
    }

    /// Decrement channel count.
    pub fn channel_destroyed(&self) {
        self.channels.fetch_sub(1, Ordering::Relaxed);
    }

    // === Getters ===

    /// Get local user count.
    pub fn local_users(&self) -> usize {
        self.local_users.load(Ordering::Relaxed)
    }

    /// Get global user count.
    pub fn global_users(&self) -> usize {
        self.global_users.load(Ordering::Relaxed)
    }

    /// Get invisible user count.
    pub fn invisible_users(&self) -> usize {
        self.invisible_users.load(Ordering::Relaxed)
    }

    /// Get local operator count.
    #[allow(dead_code)]
    pub fn local_opers(&self) -> usize {
        self.local_opers.load(Ordering::Relaxed)
    }

    /// Get global operator count.
    pub fn global_opers(&self) -> usize {
        self.global_opers.load(Ordering::Relaxed)
    }

    /// Get channel count.
    pub fn channels(&self) -> usize {
        self.channels.load(Ordering::Relaxed)
    }

    /// Get total connections since startup.
    #[allow(dead_code)]
    pub fn connections_total(&self) -> usize {
        self.connections_total.load(Ordering::Relaxed)
    }

    /// Get peak concurrent connections.
    pub fn peak_connections(&self) -> usize {
        self.peak_connections.load(Ordering::Relaxed)
    }

    /// Get peak global users.
    pub fn peak_global_users(&self) -> usize {
        self.peak_global_users.load(Ordering::Relaxed)
    }

    /// Get unregistered connection count.
    pub fn unregistered_connections(&self) -> usize {
        self.unregistered_connections.load(Ordering::Relaxed)
    }

    /// Get server uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    /// Get number of servers (1 for standalone, more with S2S).
    #[allow(dead_code)]
    pub fn servers(&self) -> usize {
        1 // TODO: integrate with SyncManager for linked servers
    }
}

impl Default for StatsManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_counters() {
        let stats = StatsManager::new();

        assert_eq!(stats.local_users(), 0);
        stats.user_connected();
        assert_eq!(stats.local_users(), 1);
        assert_eq!(stats.global_users(), 1);

        stats.user_connected();
        assert_eq!(stats.local_users(), 2);
        assert_eq!(stats.peak_connections(), 2);

        stats.user_disconnected();
        assert_eq!(stats.local_users(), 1);
        assert_eq!(stats.peak_connections(), 2); // Peak should remain
    }

    #[test]
    fn test_oper_counters() {
        let stats = StatsManager::new();

        stats.user_opered();
        assert_eq!(stats.local_opers(), 1);

        stats.user_deopered();
        assert_eq!(stats.local_opers(), 0);
    }

    #[test]
    fn test_channel_counters() {
        let stats = StatsManager::new();

        stats.channel_created();
        stats.channel_created();
        assert_eq!(stats.channels(), 2);

        stats.channel_destroyed();
        assert_eq!(stats.channels(), 1);
    }
}
