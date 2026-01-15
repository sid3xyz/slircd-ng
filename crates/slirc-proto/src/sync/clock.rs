//! Vector clocks and hybrid timestamps for causal ordering.
//!
//! This module provides time primitives for CRDT synchronization:
//! - `ServerId`: Unique identifier for a server in the cluster.
//! - `VectorClock`: Tracks causal dependencies across servers.
//! - `HybridTimestamp`: Combines wall clock and logical counter for ordering.

use std::cmp::Ordering;
use std::collections::HashMap;

/// A unique identifier for a server in the cluster.
///
/// Uses the server's SID (3 characters) for compact representation.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct ServerId(String);

impl ServerId {
    /// Create a new server ID from a SID string.
    pub fn new(sid: impl Into<String>) -> Self {
        Self(sid.into())
    }

    /// Get the inner SID string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl PartialOrd for ServerId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ServerId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

/// A hybrid logical timestamp for causal ordering.
///
/// Combines:
/// - Wall clock time (milliseconds since epoch)
/// - Logical counter (for events within the same millisecond)
/// - Server ID (for tie-breaking)
///
/// This ensures total ordering of events across the cluster.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HybridTimestamp {
    /// Wall clock time in milliseconds since Unix epoch.
    pub millis: i64,
    /// Logical counter for events within the same millisecond.
    pub counter: u32,
    /// Server ID for tie-breaking (stored as hash for compactness).
    server_hash: u64,
}

impl HybridTimestamp {
    /// Create a new timestamp.
    #[must_use]
    pub fn new(millis: i64, counter: u32, server: &ServerId) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        server.hash(&mut hasher);
        Self {
            millis,
            counter,
            server_hash: hasher.finish(),
        }
    }

    /// Create a timestamp for the current time.
    #[must_use]
    pub fn now(server: &ServerId) -> Self {
        let millis = chrono::Utc::now().timestamp_millis();
        Self::new(millis, 0, server)
    }

    /// Increment the logical counter.
    #[must_use]
    pub fn increment(&self) -> Self {
        Self {
            millis: self.millis,
            counter: self.counter.saturating_add(1),
            server_hash: self.server_hash,
        }
    }

    /// Update to be at least as recent as another timestamp.
    ///
    /// Returns a new timestamp that is causally after both.
    #[must_use]
    pub fn update(&self, other: &Self) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        let max_millis = self.millis.max(other.millis).max(now);

        let counter = if max_millis == self.millis && max_millis == other.millis {
            self.counter.max(other.counter).saturating_add(1)
        } else if max_millis == self.millis {
            self.counter.saturating_add(1)
        } else if max_millis == other.millis {
            other.counter.saturating_add(1)
        } else {
            0
        };

        Self {
            millis: max_millis,
            counter,
            server_hash: self.server_hash,
        }
    }
}

impl PartialOrd for HybridTimestamp {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HybridTimestamp {
    fn cmp(&self, other: &Self) -> Ordering {
        self.millis
            .cmp(&other.millis)
            .then(self.counter.cmp(&other.counter))
            .then(self.server_hash.cmp(&other.server_hash))
    }
}

/// A vector clock for tracking causal dependencies.
///
/// Each entry maps a server ID to its latest known timestamp.
/// Vector clocks enable detecting concurrent events and ensuring
/// causal consistency.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VectorClock {
    entries: HashMap<String, u64>,
}

impl VectorClock {
    /// Create a new empty vector clock.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current counter for a server.
    #[must_use]
    pub fn get(&self, server: &ServerId) -> u64 {
        self.entries.get(server.as_str()).copied().unwrap_or(0)
    }

    /// Increment the counter for a server.
    pub fn increment(&mut self, server: &ServerId) {
        let entry = self.entries.entry(server.as_str().to_string()).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    /// Update to include all events from another clock.
    pub fn merge(&mut self, other: &Self) {
        for (server, &counter) in &other.entries {
            let entry = self.entries.entry(server.clone()).or_insert(0);
            *entry = (*entry).max(counter);
        }
    }

    /// Check if this clock is causally before or concurrent with another.
    ///
    /// Returns:
    /// - `Less`: This clock happened-before other.
    /// - `Equal`: Clocks are equal.
    /// - `Greater`: Other clock happened-before this.
    /// - `None`: Concurrent (neither happened-before the other).
    #[must_use]
    pub fn partial_cmp_causal(&self, other: &Self) -> Option<Ordering> {
        let mut self_greater = false;
        let mut other_greater = false;

        // Check all servers in both clocks
        let all_servers: std::collections::HashSet<_> =
            self.entries.keys().chain(other.entries.keys()).collect();

        for server in all_servers {
            let self_val = self.entries.get(server).copied().unwrap_or(0);
            let other_val = other.entries.get(server).copied().unwrap_or(0);

            if self_val > other_val {
                self_greater = true;
            } else if other_val > self_val {
                other_greater = true;
            }
        }

        match (self_greater, other_greater) {
            (false, false) => Some(Ordering::Equal),
            (true, false) => Some(Ordering::Greater),
            (false, true) => Some(Ordering::Less),
            (true, true) => None, // Concurrent
        }
    }

    /// Check if this clock happened-before another.
    #[must_use]
    pub fn happened_before(&self, other: &Self) -> bool {
        matches!(self.partial_cmp_causal(other), Some(Ordering::Less))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_id_creation() {
        let server = ServerId::new("001");
        assert_eq!(server.as_str(), "001");
    }

    #[test]
    fn test_server_id_ordering() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");
        let server1_dup = ServerId::new("001");

        assert!(server1 < server2);
        assert!(server2 > server1);
        assert_eq!(server1, server1_dup);
        assert!(server1 <= server1_dup);
        assert!(server1 >= server1_dup);
    }

    #[test]
    fn test_hybrid_timestamp_ordering() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(100, 1, &server1);
        let ts3 = HybridTimestamp::new(200, 0, &server2);

        assert!(ts1 < ts2);
        assert!(ts2 < ts3);
    }

    #[test]
    fn test_hybrid_timestamp_increment() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = ts1.increment();

        assert_eq!(ts1.millis, ts2.millis);
        assert_eq!(ts2.counter, 1);
        assert!(ts1 < ts2);
    }

    #[test]
    fn test_hybrid_timestamp_increment_saturating() {
        let server = ServerId::new("001");
        let ts = HybridTimestamp::new(100, u32::MAX, &server);
        let ts_inc = ts.increment();

        // Should saturate, not overflow
        assert_eq!(ts_inc.counter, u32::MAX);
    }

    #[test]
    fn test_hybrid_timestamp_server_hash_tiebreaker() {
        // Same millis and counter, different servers
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(100, 0, &server2);

        // Must have a deterministic ordering even with same time
        assert!(ts1 != ts2);
        assert!(ts1 < ts2 || ts2 < ts1);
    }

    #[test]
    fn test_hybrid_timestamp_now() {
        let server = ServerId::new("001");
        let before = chrono::Utc::now().timestamp_millis();
        let ts = HybridTimestamp::now(&server);
        let after = chrono::Utc::now().timestamp_millis();

        assert!(ts.millis >= before);
        assert!(ts.millis <= after);
        assert_eq!(ts.counter, 0);
    }

    #[test]
    fn test_hybrid_timestamp_update_takes_max() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 5, &server1);
        let ts2 = HybridTimestamp::new(200, 3, &server2);

        // Update should produce timestamp >= both inputs
        let ts_updated = ts1.update(&ts2);
        assert!(ts_updated >= ts1);
        assert!(ts_updated >= ts2);
    }

    #[test]
    fn test_vector_clock_new_is_empty() {
        let vc = VectorClock::new();
        let server = ServerId::new("001");
        assert_eq!(vc.get(&server), 0);
    }

    #[test]
    fn test_vector_clock_increment() {
        let server = ServerId::new("001");
        let mut vc = VectorClock::new();

        assert_eq!(vc.get(&server), 0);
        vc.increment(&server);
        assert_eq!(vc.get(&server), 1);
        vc.increment(&server);
        assert_eq!(vc.get(&server), 2);
    }

    #[test]
    fn test_vector_clock_merge() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let mut vc1 = VectorClock::new();
        vc1.increment(&server1);
        vc1.increment(&server1);

        let mut vc2 = VectorClock::new();
        vc2.increment(&server2);
        vc2.increment(&server2);
        vc2.increment(&server2);

        vc1.merge(&vc2);
        assert_eq!(vc1.get(&server1), 2);
        assert_eq!(vc1.get(&server2), 3);
    }

    #[test]
    fn test_vector_clock_causality() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let mut vc1 = VectorClock::new();
        vc1.increment(&server1);
        vc1.increment(&server1);

        let mut vc2 = VectorClock::new();
        vc2.increment(&server1);
        vc2.increment(&server2);

        // vc1 and vc2 are concurrent
        assert!(vc1.partial_cmp_causal(&vc2).is_none());

        // Merge creates clock that dominates both
        let mut vc3 = vc1.clone();
        vc3.merge(&vc2);

        assert!(matches!(vc1.partial_cmp_causal(&vc3), Some(Ordering::Less)));
        assert!(matches!(vc2.partial_cmp_causal(&vc3), Some(Ordering::Less)));
    }

    #[test]
    fn test_vector_clock_happened_before() {
        let server = ServerId::new("001");

        let mut vc1 = VectorClock::new();
        vc1.increment(&server);

        let mut vc2 = vc1.clone();
        vc2.increment(&server);

        assert!(vc1.happened_before(&vc2));
        assert!(!vc2.happened_before(&vc1));
        assert!(!vc1.happened_before(&vc1)); // Self is not before self
    }

    #[test]
    fn test_vector_clock_equal() {
        let server = ServerId::new("001");

        let mut vc1 = VectorClock::new();
        vc1.increment(&server);

        let mut vc2 = VectorClock::new();
        vc2.increment(&server);

        assert!(matches!(
            vc1.partial_cmp_causal(&vc2),
            Some(Ordering::Equal)
        ));
    }
}
