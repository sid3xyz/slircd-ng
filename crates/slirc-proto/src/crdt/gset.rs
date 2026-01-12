//! Grow-only set (G-Set) CRDT.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hash::Hash;

/// A grow-only set that supports add operations but never removes.
///
/// G-Sets are the simplest set CRDT. Elements can only be added, never removed.
/// Merge is simply set union, which is commutative, associative, and idempotent.
///
/// # Example
///
/// ```rust
/// use slirc_proto::crdt::GSet;
///
/// let mut set_a: GSet<&str> = GSet::new();
/// let mut set_b: GSet<&str> = GSet::new();
///
/// set_a.insert("alice");
/// set_a.insert("bob");
/// set_b.insert("bob");
/// set_b.insert("charlie");
///
/// set_a.merge(&set_b);
/// assert!(set_a.contains(&"alice"));
/// assert!(set_a.contains(&"bob"));
/// assert!(set_a.contains(&"charlie"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GSet<T>
where
    T: Eq + Hash,
{
    elements: HashSet<T>,
}

impl<T> Default for GSet<T>
where
    T: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> GSet<T>
where
    T: Eq + Hash,
{
    /// Creates a new empty G-Set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            elements: HashSet::new(),
        }
    }

    /// Inserts an element into the set.
    ///
    /// Returns `true` if the element was not already present.
    pub fn insert(&mut self, value: T) -> bool {
        self.elements.insert(value)
    }

    /// Returns `true` if the set contains the given value.
    pub fn contains(&self, value: &T) -> bool {
        self.elements.contains(value)
    }

    /// Returns the number of elements in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements.len()
    }

    /// Returns `true` if the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Returns an iterator over the elements.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements.iter()
    }

    /// Merges another G-Set into this one (set union).
    ///
    /// This operation is:
    /// - Commutative: `a.merge(b)` equivalent to `b.merge(a)` in result
    /// - Associative: `(a.merge(b)).merge(c)` == `a.merge(b.merge(c))`
    /// - Idempotent: `a.merge(a)` == `a`
    pub fn merge(&mut self, other: &GSet<T>)
    where
        T: Clone,
    {
        for elem in &other.elements {
            self.elements.insert(elem.clone());
        }
    }

    /// Consumes another G-Set and merges it into this one.
    pub fn merge_owned(&mut self, other: GSet<T>) {
        self.elements.extend(other.elements);
    }
}

impl<T> FromIterator<T> for GSet<T>
where
    T: Eq + Hash,
{
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            elements: iter.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_set_is_empty() {
        let set: GSet<i32> = GSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_insert_and_contains() {
        let mut set = GSet::new();
        assert!(!set.contains(&"hello"));

        assert!(set.insert("hello")); // new element
        assert!(set.contains(&"hello"));
        assert_eq!(set.len(), 1);

        assert!(!set.insert("hello")); // duplicate
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn test_merge_union() {
        let mut set_a: GSet<i32> = GSet::new();
        let mut set_b: GSet<i32> = GSet::new();

        set_a.insert(1);
        set_a.insert(2);
        set_b.insert(2);
        set_b.insert(3);

        set_a.merge(&set_b);

        assert!(set_a.contains(&1));
        assert!(set_a.contains(&2));
        assert!(set_a.contains(&3));
        assert_eq!(set_a.len(), 3);
    }

    #[test]
    fn test_merge_is_commutative() {
        let set_a: GSet<i32> = [1, 2].into_iter().collect();
        let set_b: GSet<i32> = [2, 3].into_iter().collect();

        let mut result_ab = set_a.clone();
        result_ab.merge(&set_b);

        let mut result_ba = set_b.clone();
        result_ba.merge(&set_a);

        assert_eq!(result_ab, result_ba);
    }

    #[test]
    fn test_merge_is_idempotent() {
        let mut set: GSet<i32> = [1, 2, 3].into_iter().collect();
        let original = set.clone();

        set.merge(&original);
        assert_eq!(set, original);
    }

    #[test]
    fn test_merge_is_associative() {
        let set_a: GSet<i32> = [1].into_iter().collect();
        let set_b: GSet<i32> = [2].into_iter().collect();
        let set_c: GSet<i32> = [3].into_iter().collect();

        // (a merge b) merge c
        let mut result_1 = set_a.clone();
        result_1.merge(&set_b);
        result_1.merge(&set_c);

        // a merge (b merge c)
        let mut bc = set_b.clone();
        bc.merge(&set_c);
        let mut result_2 = set_a.clone();
        result_2.merge(&bc);

        assert_eq!(result_1, result_2);
    }

    #[test]
    fn test_iter() {
        let set: GSet<i32> = [1, 2, 3].into_iter().collect();
        let collected: HashSet<i32> = set.iter().copied().collect();
        assert_eq!(collected, [1, 2, 3].into_iter().collect());
    }

    #[test]
    #[cfg(feature = "serde")]
    fn test_serde_roundtrip() {
        let set: GSet<String> = ["a", "b", "c"].into_iter().map(String::from).collect();
        let serialized = serde_json::to_string(&set).unwrap();
        let deserialized: GSet<String> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(set, deserialized);
    }
}
