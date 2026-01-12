//! Observed-Remove Set (OR-Set) CRDT.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::Hash;
use uuid::Uuid;

/// An Observed-Remove Set that supports both add and remove operations.
///
/// Each element is tagged with a unique identifier (UUID). When an element
/// is removed, only the currently observed tags are removed. Concurrent adds
/// of the same value survive because they have different tags.
///
/// # Semantics
///
/// - `insert(e)`: Adds element `e` with a fresh unique tag
/// - `remove(e)`: Removes all currently observed tags for `e`
/// - Concurrent add/remove: Add wins (new tag not in removed set)
/// - Merge: Union of all (element, tag) pairs minus removed tags
///
/// # Example
///
/// ```rust
/// use slirc_proto::crdt::ORSet;
///
/// let mut set_a: ORSet<&str> = ORSet::new();
/// let mut set_b: ORSet<&str> = ORSet::new();
///
/// // Both add "alice"
/// set_a.insert("alice");
/// set_b.insert("alice");
///
/// // Node A removes alice
/// set_a.remove(&"alice");
/// assert!(!set_a.contains(&"alice"));
///
/// // Merge: B's add of alice survives because it has a different tag
/// set_a.merge(&set_b);
/// assert!(set_a.contains(&"alice"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ORSet<T>
where
    T: Eq + Hash,
{
    /// Maps elements to their unique tags
    elements: HashMap<T, HashSet<Uuid>>,
}

impl<T> Default for ORSet<T>
where
    T: Eq + Hash,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ORSet<T>
where
    T: Eq + Hash,
{
    /// Creates a new empty OR-Set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
        }
    }

    /// Inserts an element with a fresh unique tag.
    ///
    /// Returns the UUID tag assigned to this insertion.
    pub fn insert(&mut self, value: T) -> Uuid
    where
        T: Clone,
    {
        let tag = Uuid::new_v4();
        self.insert_with_tag(value, tag);
        tag
    }

    /// Inserts an element with a specific tag (used during merge).
    fn insert_with_tag(&mut self, value: T, tag: Uuid)
    where
        T: Clone,
    {
        self.elements.entry(value).or_default().insert(tag);
    }

    /// Removes an element by removing all its observed tags.
    ///
    /// Returns the set of tags that were removed, or `None` if the element
    /// was not present.
    pub fn remove(&mut self, value: &T) -> Option<HashSet<Uuid>> {
        self.elements.remove(value)
    }

    /// Returns `true` if the set contains the given value (with at least one tag).
    pub fn contains(&self, value: &T) -> bool {
        self.elements
            .get(value)
            .is_some_and(|tags| !tags.is_empty())
    }

    /// Returns the number of unique elements in the set.
    #[must_use]
    pub fn len(&self) -> usize {
        self.elements
            .values()
            .filter(|tags| !tags.is_empty())
            .count()
    }

    /// Returns `true` if the set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.elements.values().all(|tags| tags.is_empty())
    }

    /// Returns an iterator over the elements.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.elements
            .iter()
            .filter(|(_, tags)| !tags.is_empty())
            .map(|(elem, _)| elem)
    }

    /// Returns the tags associated with an element.
    pub fn tags(&self, value: &T) -> Option<&HashSet<Uuid>> {
        self.elements.get(value).filter(|tags| !tags.is_empty())
    }

    /// Merges another OR-Set into this one.
    ///
    /// For each element, takes the union of tags from both sets.
    /// Elements are present if they have at least one tag.
    pub fn merge(&mut self, other: &ORSet<T>)
    where
        T: Clone,
    {
        for (elem, other_tags) in &other.elements {
            let self_tags = self.elements.entry(elem.clone()).or_default();
            self_tags.extend(other_tags.iter().copied());
        }
    }

    /// Merges and removes specific tags (for handling remote removes).
    ///
    /// This is used when receiving a remove operation that specifies
    /// which tags to remove.
    pub fn remove_tags(&mut self, value: &T, tags_to_remove: &HashSet<Uuid>) {
        if let Some(self_tags) = self.elements.get_mut(value) {
            for tag in tags_to_remove {
                self_tags.remove(tag);
            }
            // Clean up empty entries
            if self_tags.is_empty() {
                self.elements.remove(value);
            }
        }
    }

    /// Returns all elements with their tags (for serialization/sync).
    pub fn elements_with_tags(&self) -> impl Iterator<Item = (&T, &HashSet<Uuid>)> {
        self.elements.iter().filter(|(_, tags)| !tags.is_empty())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_set_is_empty() {
        let set: ORSet<i32> = ORSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_insert_and_contains() {
        let mut set = ORSet::new();
        assert!(!set.contains(&"hello"));

        let tag = set.insert("hello");
        assert!(set.contains(&"hello"));
        assert_eq!(set.len(), 1);

        // Verify tag was assigned
        let tags = set.tags(&"hello").unwrap();
        assert!(tags.contains(&tag));
    }

    #[test]
    fn test_insert_same_value_multiple_times() {
        let mut set = ORSet::new();
        let tag1 = set.insert("hello");
        let tag2 = set.insert("hello");

        assert_ne!(tag1, tag2); // Different tags
        assert_eq!(set.len(), 1); // Still one element

        let tags = set.tags(&"hello").unwrap();
        assert_eq!(tags.len(), 2);
        assert!(tags.contains(&tag1));
        assert!(tags.contains(&tag2));
    }

    #[test]
    fn test_remove() {
        let mut set = ORSet::new();
        set.insert("hello");
        set.insert("hello"); // Two tags
        assert!(set.contains(&"hello"));

        let removed_tags = set.remove(&"hello");
        assert!(removed_tags.is_some());
        assert_eq!(removed_tags.unwrap().len(), 2);
        assert!(!set.contains(&"hello"));
        assert!(set.is_empty());
    }

    #[test]
    fn test_remove_nonexistent() {
        let mut set: ORSet<i32> = ORSet::new();
        assert!(set.remove(&42).is_none());
    }

    #[test]
    fn test_merge_union() {
        let mut set_a: ORSet<i32> = ORSet::new();
        let mut set_b: ORSet<i32> = ORSet::new();

        set_a.insert(1);
        set_a.insert(2);
        set_b.insert(2);
        set_b.insert(3);

        set_a.merge(&set_b);

        assert!(set_a.contains(&1));
        assert!(set_a.contains(&2));
        assert!(set_a.contains(&3));
        assert_eq!(set_a.len(), 3);

        // Element 2 should have tags from both sets
        let tags_2 = set_a.tags(&2).unwrap();
        assert_eq!(tags_2.len(), 2);
    }

    #[test]
    fn test_concurrent_add_remove_add_wins() {
        let mut set_a: ORSet<&str> = ORSet::new();
        let mut set_b: ORSet<&str> = ORSet::new();

        // Both add "alice"
        set_a.insert("alice");
        set_b.insert("alice");

        // A removes alice (removes A's tag only, doesn't know about B's tag)
        set_a.remove(&"alice");
        assert!(!set_a.contains(&"alice"));

        // Merge: B's tag survives
        set_a.merge(&set_b);
        assert!(set_a.contains(&"alice"));
    }

    #[test]
    fn test_remove_tags() {
        let mut set = ORSet::new();
        let tag1 = set.insert("hello");
        let tag2 = set.insert("hello");

        let mut tags_to_remove = HashSet::new();
        tags_to_remove.insert(tag1);

        set.remove_tags(&"hello", &tags_to_remove);

        // Still contains because tag2 remains
        assert!(set.contains(&"hello"));
        let remaining = set.tags(&"hello").unwrap();
        assert_eq!(remaining.len(), 1);
        assert!(remaining.contains(&tag2));

        // Remove remaining tag
        let mut more_tags = HashSet::new();
        more_tags.insert(tag2);
        set.remove_tags(&"hello", &more_tags);

        assert!(!set.contains(&"hello"));
    }

    #[test]
    fn test_merge_is_commutative() {
        let mut set_a: ORSet<i32> = ORSet::new();
        let mut set_b: ORSet<i32> = ORSet::new();

        set_a.insert(1);
        set_b.insert(2);

        let mut result_ab = set_a.clone();
        result_ab.merge(&set_b);

        let mut result_ba = set_b.clone();
        result_ba.merge(&set_a);

        // Both should contain the same elements
        assert_eq!(result_ab.len(), result_ba.len());
        assert!(result_ab.contains(&1));
        assert!(result_ab.contains(&2));
        assert!(result_ba.contains(&1));
        assert!(result_ba.contains(&2));
    }

    #[test]
    fn test_merge_is_idempotent() {
        let mut set: ORSet<i32> = ORSet::new();
        set.insert(1);
        set.insert(2);

        let original = set.clone();
        set.merge(&original);

        assert_eq!(set.len(), original.len());
        // Tags should be the same (merging adds no new tags)
        assert_eq!(set.tags(&1), original.tags(&1));
        assert_eq!(set.tags(&2), original.tags(&2));
    }

    #[test]
    fn test_iter() {
        let mut set: ORSet<i32> = ORSet::new();
        set.insert(1);
        set.insert(2);
        set.insert(3);

        let collected: HashSet<i32> = set.iter().copied().collect();
        assert_eq!(collected, [1, 2, 3].into_iter().collect());
    }

    #[test]
    fn test_elements_with_tags() {
        let mut set: ORSet<i32> = ORSet::new();
        let tag1 = set.insert(1);
        let tag2 = set.insert(2);

        let elements: Vec<_> = set.elements_with_tags().collect();
        assert_eq!(elements.len(), 2);

        for (elem, tags) in elements {
            if *elem == 1 {
                assert!(tags.contains(&tag1));
            } else if *elem == 2 {
                assert!(tags.contains(&tag2));
            }
        }
    }

    #[test]
    #[cfg(feature = "serde")]
    fn test_serde_roundtrip() {
        let mut set: ORSet<String> = ORSet::new();
        set.insert("a".to_string());
        set.insert("b".to_string());

        let serialized = serde_json::to_string(&set).unwrap();
        let deserialized: ORSet<String> = serde_json::from_str(&serialized).unwrap();

        assert!(deserialized.contains(&"a".to_string()));
        assert!(deserialized.contains(&"b".to_string()));
        assert_eq!(deserialized.len(), 2);
    }
}
