//! Core CRDT traits for distributed state synchronization.
//!
//! These traits define the interface that all CRDT types must implement
//! to participate in server-to-server state synchronization.

use super::clock::HybridTimestamp;

/// A Conflict-free Replicated Data Type.
///
/// CRDTs support commutative, associative, and idempotent merge operations,
/// ensuring eventual consistency without coordination.
pub trait Crdt: Sized {
    /// Merge another instance into this one.
    ///
    /// This operation must be:
    /// - **Commutative**: `a.merge(b)` == `b.merge(a)`
    /// - **Associative**: `a.merge(b.merge(c))` == `a.merge(b).merge(c)`
    /// - **Idempotent**: `a.merge(a)` == `a`
    fn merge(&mut self, other: &Self);

    /// Check if this instance is causally greater than or equal to another.
    ///
    /// Returns `true` if merging `other` into `self` would not change `self`.
    fn dominates(&self, other: &Self) -> bool;
}

/// A type that can produce and apply incremental deltas.
///
/// Deltas are more efficient for network transfer than full state.
pub trait StateDelta: Crdt {
    /// The delta type for incremental updates.
    type Delta: Clone + serde::Serialize + for<'de> serde::Deserialize<'de>;

    /// Generate a delta representing changes since a given timestamp.
    fn delta_since(&self, since: HybridTimestamp) -> Option<Self::Delta>;

    /// Apply a delta to this instance.
    fn apply_delta(&mut self, delta: &Self::Delta);
}

/// A value with an associated timestamp for Last-Writer-Wins semantics.
///
/// When merging, the value with the higher timestamp wins. Ties are broken
/// by lexicographic comparison of server IDs to ensure determinism.
pub trait Mergeable: Clone {
    /// Get the timestamp of this value.
    fn timestamp(&self) -> HybridTimestamp;

    /// Merge with another value, returning the winner.
    #[must_use]
    fn merge_with(&self, other: &Self) -> Self {
        if other.timestamp() > self.timestamp() {
            other.clone()
        } else {
            self.clone()
        }
    }
}

/// A Last-Writer-Wins register.
///
/// Wraps any value with a timestamp for LWW semantics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LwwRegister<T> {
    value: T,
    timestamp: HybridTimestamp,
}

impl<T: Clone> LwwRegister<T> {
    /// Create a new LWW register with the given value and timestamp.
    pub fn new(value: T, timestamp: HybridTimestamp) -> Self {
        Self { value, timestamp }
    }

    /// Get the current value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Get the timestamp.
    pub fn timestamp(&self) -> HybridTimestamp {
        self.timestamp
    }

    /// Update the value if the new timestamp is greater.
    pub fn update(&mut self, value: T, timestamp: HybridTimestamp) {
        if timestamp > self.timestamp {
            self.value = value;
            self.timestamp = timestamp;
        }
    }
}

impl<T: Clone> Mergeable for LwwRegister<T> {
    fn timestamp(&self) -> HybridTimestamp {
        self.timestamp
    }
}

impl<T: Clone> Crdt for LwwRegister<T> {
    fn merge(&mut self, other: &Self) {
        if other.timestamp > self.timestamp {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
        }
    }

    fn dominates(&self, other: &Self) -> bool {
        self.timestamp >= other.timestamp
    }
}

/// An Add-Wins Set (`AWSet`) for sets where adds take precedence.
///
/// When an add and remove happen concurrently, the add wins.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AwSet<T>
where
    T: Clone + Eq + std::hash::Hash,
{
    /// Elements with their add timestamps.
    elements: std::collections::HashMap<T, HybridTimestamp>,
    /// Tombstones for removed elements (removed at timestamp).
    tombstones: std::collections::HashMap<T, HybridTimestamp>,
}

impl<T> Default for AwSet<T>
where
    T: Clone + Eq + std::hash::Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AwSet<T>
where
    T: Clone + Eq + std::hash::Hash,
{
    /// Create an empty `AWSet`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            elements: std::collections::HashMap::new(),
            tombstones: std::collections::HashMap::new(),
        }
    }

    /// Add an element with the given timestamp.
    pub fn add(&mut self, element: T, timestamp: HybridTimestamp) {
        // Add wins: if add timestamp >= tombstone timestamp, element is present
        let tombstone_ts = self.tombstones.get(&element).copied();
        if tombstone_ts.map_or(true, |ts| timestamp >= ts) {
            self.elements.insert(element, timestamp);
        }
    }

    /// Remove an element with the given timestamp.
    pub fn remove(&mut self, element: &T, timestamp: HybridTimestamp) {
        // Only remove if tombstone timestamp > add timestamp
        if let Some(&add_ts) = self.elements.get(element) {
            if timestamp > add_ts {
                self.elements.remove(element);
                self.tombstones.insert(element.clone(), timestamp);
            }
        }
    }

    /// Check if an element is present.
    pub fn contains(&self, element: &T) -> bool {
        self.elements.contains_key(element)
    }

    /// Iterate over present elements.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements.keys()
    }

    /// Get the number of elements.
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Check if the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

impl<T> Crdt for AwSet<T>
where
    T: Clone + Eq + std::hash::Hash,
{
    fn merge(&mut self, other: &Self) {
        // Merge elements: take the later timestamp for each
        for (elem, &other_ts) in &other.elements {
            match self.elements.get(elem) {
                Some(&self_ts) if self_ts >= other_ts => {}
                _ => {
                    // Check against our tombstones
                    let our_tomb = self.tombstones.get(elem).copied();
                    if our_tomb.map_or(true, |ts| other_ts >= ts) {
                        self.elements.insert(elem.clone(), other_ts);
                    }
                }
            }
        }

        // Merge tombstones: take the later timestamp
        for (elem, &other_ts) in &other.tombstones {
            match self.tombstones.get(elem) {
                Some(&self_ts) if self_ts >= other_ts => {}
                _ => {
                    self.tombstones.insert(elem.clone(), other_ts);
                    // Apply tombstone: remove if add timestamp < tombstone
                    if let Some(&add_ts) = self.elements.get(elem) {
                        if other_ts > add_ts {
                            self.elements.remove(elem);
                        }
                    }
                }
            }
        }
    }

    fn dominates(&self, other: &Self) -> bool {
        // We dominate if all of other's elements and tombstones are
        // present with equal or greater timestamps
        for (elem, &other_ts) in &other.elements {
            match self.elements.get(elem) {
                Some(&self_ts) if self_ts >= other_ts => {}
                _ => return false,
            }
        }
        for (elem, &other_ts) in &other.tombstones {
            match self.tombstones.get(elem) {
                Some(&self_ts) if self_ts >= other_ts => {}
                _ => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::ServerId;

    #[test]
    fn test_lww_register_new() {
        let server = ServerId::new("001");
        let ts = HybridTimestamp::new(100, 0, &server);

        let reg: LwwRegister<String> = LwwRegister::new("value".to_string(), ts);
        assert_eq!(*reg.value(), "value");
        assert_eq!(reg.timestamp(), ts);
    }

    #[test]
    fn test_lww_register_update_newer_wins() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);

        let mut reg = LwwRegister::new("old", ts1);
        reg.update("new", ts2);
        assert_eq!(*reg.value(), "new");
    }

    #[test]
    fn test_lww_register_update_older_ignored() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);

        let mut reg = LwwRegister::new("newer", ts2);
        reg.update("older", ts1); // This should be ignored
        assert_eq!(*reg.value(), "newer");
    }

    #[test]
    fn test_lww_register_merge() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(200, 0, &server2);

        let mut reg1 = LwwRegister::new("old", ts1);
        let reg2 = LwwRegister::new("new", ts2);

        reg1.merge(&reg2);
        assert_eq!(*reg1.value(), "new");
    }

    #[test]
    fn test_lww_register_merge_older_ignored() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(200, 0, &server2);

        let mut reg1 = LwwRegister::new("newer", ts2);
        let reg2 = LwwRegister::new("older", ts1);

        reg1.merge(&reg2);
        assert_eq!(*reg1.value(), "newer");
    }

    #[test]
    fn test_lww_register_dominates() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);

        let reg1 = LwwRegister::new("value", ts1);
        let reg2 = LwwRegister::new("value", ts2);

        assert!(reg2.dominates(&reg1)); // Newer dominates older
        assert!(!reg1.dominates(&reg2)); // Older doesn't dominate newer
        assert!(reg1.dominates(&reg1)); // Self dominates self
    }

    #[test]
    fn test_lww_register_mergeable() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);

        let reg1 = LwwRegister::new("old", ts1);
        let reg2 = LwwRegister::new("new", ts2);

        let merged = reg1.merge_with(&reg2);
        assert_eq!(*merged.value(), "new");

        let merged_reverse = reg2.merge_with(&reg1);
        assert_eq!(*merged_reverse.value(), "new");
    }

    #[test]
    fn test_awset_new_is_empty() {
        let set: AwSet<String> = AwSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_awset_add_contains() {
        let server = ServerId::new("001");
        let ts = HybridTimestamp::new(100, 0, &server);

        let mut set: AwSet<String> = AwSet::new();
        set.add("item".to_string(), ts);

        assert!(set.contains(&"item".to_string()));
        assert!(!set.contains(&"other".to_string()));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_awset_add_wins() {
        let server1 = ServerId::new("001");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(100, 0, &server1); // Same timestamp

        let mut set: AwSet<String> = AwSet::new();
        set.add("user".to_string(), ts1);
        set.remove(&"user".to_string(), ts2);

        // Add-wins: same timestamp means add wins
        assert!(set.contains(&"user".to_string()));
    }

    #[test]
    fn test_awset_remove_after_add() {
        let server1 = ServerId::new("001");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(200, 0, &server1); // Later timestamp

        let mut set: AwSet<String> = AwSet::new();
        set.add("user".to_string(), ts1);
        set.remove(&"user".to_string(), ts2);

        // Remove with later timestamp succeeds
        assert!(!set.contains(&"user".to_string()));
    }

    #[test]
    fn test_awset_iter() {
        let server = ServerId::new("001");
        let ts = HybridTimestamp::new(100, 0, &server);

        let mut set: AwSet<i32> = AwSet::new();
        set.add(1, ts);
        set.add(2, ts);
        set.add(3, ts);

        let items: Vec<_> = set.iter().copied().collect();
        assert_eq!(items.len(), 3);
        assert!(items.contains(&1));
        assert!(items.contains(&2));
        assert!(items.contains(&3));
    }

    #[test]
    fn test_awset_merge_concurrent_adds() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts1 = HybridTimestamp::new(100, 0, &server1);
        let ts2 = HybridTimestamp::new(100, 0, &server2);

        let mut set1: AwSet<String> = AwSet::new();
        set1.add("item1".to_string(), ts1);

        let mut set2: AwSet<String> = AwSet::new();
        set2.add("item2".to_string(), ts2);

        set1.merge(&set2);

        // Both items should be present after merge
        assert!(set1.contains(&"item1".to_string()));
        assert!(set1.contains(&"item2".to_string()));
    }

    #[test]
    fn test_awset_merge_add_remove_conflict() {
        let server1 = ServerId::new("001");
        let server2 = ServerId::new("002");

        let ts_add = HybridTimestamp::new(100, 0, &server1);
        let ts_remove = HybridTimestamp::new(200, 0, &server2);
        let ts_readd = HybridTimestamp::new(300, 0, &server1);

        let mut set1: AwSet<String> = AwSet::new();
        set1.add("item".to_string(), ts_add);
        set1.remove(&"item".to_string(), ts_remove);

        let mut set2: AwSet<String> = AwSet::new();
        set2.add("item".to_string(), ts_readd);

        set1.merge(&set2);

        // Re-add at later timestamp should win
        assert!(set1.contains(&"item".to_string()));
    }

    #[test]
    fn test_awset_dominates() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);

        let mut set1: AwSet<String> = AwSet::new();
        set1.add("item".to_string(), ts1);

        let mut set2: AwSet<String> = AwSet::new();
        set2.add("item".to_string(), ts2);

        // set2 has later timestamp, so it dominates
        assert!(set2.dominates(&set1));
        assert!(!set1.dominates(&set2));
    }

    #[test]
    fn test_awset_remove_nonexistent_ignored() {
        let server = ServerId::new("001");
        let ts = HybridTimestamp::new(100, 0, &server);

        let mut set: AwSet<String> = AwSet::new();
        set.remove(&"nonexistent".to_string(), ts);

        // Should not cause issues
        assert!(!set.contains(&"nonexistent".to_string()));
        assert!(set.is_empty());
    }

    #[test]
    fn test_awset_re_add_after_tombstone() {
        let server = ServerId::new("001");
        let ts1 = HybridTimestamp::new(100, 0, &server);
        let ts2 = HybridTimestamp::new(200, 0, &server);
        let ts3 = HybridTimestamp::new(300, 0, &server);

        let mut set: AwSet<String> = AwSet::new();
        set.add("item".to_string(), ts1);
        set.remove(&"item".to_string(), ts2);
        assert!(!set.contains(&"item".to_string()));

        set.add("item".to_string(), ts3);
        assert!(set.contains(&"item".to_string()));
    }
}
