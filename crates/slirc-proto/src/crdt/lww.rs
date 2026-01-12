//! Last-Writer-Wins (LWW) Register CRDT.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A Last-Writer-Wins register that resolves conflicts by timestamp.
///
/// When two nodes concurrently update the register, the update with the
/// higher timestamp wins. This provides eventual consistency at the cost
/// of potentially losing concurrent updates.
///
/// # Example
///
/// ```rust
/// use slirc_proto::crdt::LwwRegister;
///
/// let mut reg_a = LwwRegister::new("initial", 1);
/// let mut reg_b = LwwRegister::new("initial", 1);
///
/// // Concurrent updates with different timestamps
/// reg_a.set("value_a", 5);
/// reg_b.set("value_b", 10);
///
/// // Merge: higher timestamp wins
/// reg_a.merge(&reg_b);
/// assert_eq!(reg_a.get(), &"value_b");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LwwRegister<T> {
    value: T,
    timestamp: u64,
}

impl<T> LwwRegister<T> {
    /// Creates a new LWW register with an initial value and timestamp.
    #[must_use]
    pub const fn new(value: T, timestamp: u64) -> Self {
        Self { value, timestamp }
    }

    /// Returns a reference to the current value.
    #[must_use]
    pub const fn get(&self) -> &T {
        &self.value
    }

    /// Returns the current timestamp.
    #[must_use]
    pub const fn timestamp(&self) -> u64 {
        self.timestamp
    }

    /// Sets a new value with the given timestamp.
    ///
    /// The update is only applied if the new timestamp is greater than
    /// the current timestamp.
    ///
    /// Returns `true` if the value was updated.
    pub fn set(&mut self, value: T, timestamp: u64) -> bool {
        if timestamp > self.timestamp {
            self.value = value;
            self.timestamp = timestamp;
            true
        } else {
            false
        }
    }

    /// Sets a new value, automatically incrementing the timestamp.
    ///
    /// This is useful for local updates where you want to ensure the
    /// new value takes precedence.
    pub fn set_local(&mut self, value: T) {
        self.timestamp += 1;
        self.value = value;
    }

    /// Merges another register into this one.
    ///
    /// The value with the higher timestamp wins. If timestamps are equal,
    /// the current value is kept (arbitrary but deterministic tie-breaker).
    ///
    /// Returns `true` if this register's value was updated.
    pub fn merge(&mut self, other: &LwwRegister<T>) -> bool
    where
        T: Clone,
    {
        if other.timestamp > self.timestamp {
            self.value = other.value.clone();
            self.timestamp = other.timestamp;
            true
        } else {
            false
        }
    }

    /// Consumes another register and merges it into this one.
    pub fn merge_owned(&mut self, other: LwwRegister<T>) -> bool {
        if other.timestamp > self.timestamp {
            self.value = other.value;
            self.timestamp = other.timestamp;
            true
        } else {
            false
        }
    }

    /// Unwraps the register, returning the inner value.
    #[must_use]
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: Default> Default for LwwRegister<T> {
    fn default() -> Self {
        Self::new(T::default(), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_and_get() {
        let reg = LwwRegister::new("hello", 42);
        assert_eq!(reg.get(), &"hello");
        assert_eq!(reg.timestamp(), 42);
    }

    #[test]
    fn test_set_with_higher_timestamp() {
        let mut reg = LwwRegister::new("old", 5);
        assert!(reg.set("new", 10));
        assert_eq!(reg.get(), &"new");
        assert_eq!(reg.timestamp(), 10);
    }

    #[test]
    fn test_set_with_lower_timestamp_ignored() {
        let mut reg = LwwRegister::new("current", 10);
        assert!(!reg.set("older", 5));
        assert_eq!(reg.get(), &"current");
        assert_eq!(reg.timestamp(), 10);
    }

    #[test]
    fn test_set_with_equal_timestamp_ignored() {
        let mut reg = LwwRegister::new("current", 10);
        assert!(!reg.set("concurrent", 10));
        assert_eq!(reg.get(), &"current");
    }

    #[test]
    fn test_set_local() {
        let mut reg = LwwRegister::new("a", 5);
        reg.set_local("b");
        assert_eq!(reg.get(), &"b");
        assert_eq!(reg.timestamp(), 6);

        reg.set_local("c");
        assert_eq!(reg.get(), &"c");
        assert_eq!(reg.timestamp(), 7);
    }

    #[test]
    fn test_merge_higher_timestamp_wins() {
        let mut reg_a = LwwRegister::new("a", 5);
        let reg_b = LwwRegister::new("b", 10);

        assert!(reg_a.merge(&reg_b));
        assert_eq!(reg_a.get(), &"b");
        assert_eq!(reg_a.timestamp(), 10);
    }

    #[test]
    fn test_merge_lower_timestamp_ignored() {
        let mut reg_a = LwwRegister::new("a", 10);
        let reg_b = LwwRegister::new("b", 5);

        assert!(!reg_a.merge(&reg_b));
        assert_eq!(reg_a.get(), &"a");
        assert_eq!(reg_a.timestamp(), 10);
    }

    #[test]
    fn test_merge_equal_timestamp_keeps_current() {
        let mut reg_a = LwwRegister::new("a", 10);
        let reg_b = LwwRegister::new("b", 10);

        assert!(!reg_a.merge(&reg_b));
        assert_eq!(reg_a.get(), &"a");
    }

    #[test]
    fn test_merge_is_commutative_in_result() {
        // When one clearly wins, both directions should get the same final value
        let reg_a = LwwRegister::new("a", 5);
        let reg_b = LwwRegister::new("b", 10);

        let mut result_ab = reg_a.clone();
        result_ab.merge(&reg_b);

        let mut result_ba = reg_b.clone();
        result_ba.merge(&reg_a);

        // Both should have the higher-timestamp value
        assert_eq!(result_ab.get(), result_ba.get());
        assert_eq!(result_ab.timestamp(), result_ba.timestamp());
    }

    #[test]
    fn test_into_inner() {
        let reg = LwwRegister::new(String::from("value"), 1);
        let value = reg.into_inner();
        assert_eq!(value, "value");
    }

    #[test]
    fn test_default() {
        let reg: LwwRegister<i32> = LwwRegister::default();
        assert_eq!(reg.get(), &0);
        assert_eq!(reg.timestamp(), 0);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn test_serde_roundtrip() {
        let reg = LwwRegister::new("test_value", 42);
        let serialized = serde_json::to_string(&reg).unwrap();
        let deserialized: LwwRegister<&str> = serde_json::from_str(&serialized).unwrap();
        assert_eq!(reg.get(), deserialized.get());
        assert_eq!(reg.timestamp(), deserialized.timestamp());
    }
}
