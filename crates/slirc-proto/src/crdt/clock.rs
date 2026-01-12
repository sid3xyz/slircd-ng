//! Lamport logical clock for ordering distributed events.

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A Lamport logical clock for establishing causal ordering of events.
///
/// Each node maintains its own clock. The clock value increases monotonically
/// and is updated on local events and when receiving messages from other nodes.
///
/// # Example
///
/// ```rust
/// use slirc_proto::crdt::LamportClock;
///
/// let mut clock_a = LamportClock::new();
/// let mut clock_b = LamportClock::new();
///
/// // Node A performs some events
/// clock_a.tick();
/// clock_a.tick();
/// assert_eq!(clock_a.value(), 2);
///
/// // Node B receives a message from A and merges
/// clock_b.merge(&clock_a);
/// assert_eq!(clock_b.value(), 3); // max(0, 2) + 1 = 3
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LamportClock {
    value: u64,
}

impl Default for LamportClock {
    fn default() -> Self {
        Self::new()
    }
}

impl LamportClock {
    /// Creates a new clock initialized to 0.
    #[must_use]
    pub const fn new() -> Self {
        Self { value: 0 }
    }

    /// Creates a clock with a specific initial value.
    #[must_use]
    pub const fn with_value(value: u64) -> Self {
        Self { value }
    }

    /// Returns the current clock value.
    #[must_use]
    pub const fn value(&self) -> u64 {
        self.value
    }

    /// Increments the clock for a local event and returns the new value.
    pub fn tick(&mut self) -> u64 {
        self.value += 1;
        self.value
    }

    /// Merges with another clock (typically from a received message).
    ///
    /// Sets this clock to `max(self, other) + 1` to maintain causal ordering.
    pub fn merge(&mut self, other: &LamportClock) -> u64 {
        self.value = self.value.max(other.value) + 1;
        self.value
    }

    /// Compares two clocks for happens-before relationship.
    ///
    /// Returns `true` if `self` definitely happened before `other`.
    /// Note: Lamport clocks only provide partial ordering - if this returns
    /// `false`, it doesn't mean `other` happened before `self`.
    #[must_use]
    pub const fn happened_before(&self, other: &LamportClock) -> bool {
        self.value < other.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_clock_starts_at_zero() {
        let clock = LamportClock::new();
        assert_eq!(clock.value(), 0);
    }

    #[test]
    fn test_tick_increments() {
        let mut clock = LamportClock::new();
        assert_eq!(clock.tick(), 1);
        assert_eq!(clock.tick(), 2);
        assert_eq!(clock.tick(), 3);
        assert_eq!(clock.value(), 3);
    }

    #[test]
    fn test_merge_takes_max_plus_one() {
        let mut clock_a = LamportClock::with_value(5);
        let clock_b = LamportClock::with_value(3);

        clock_a.merge(&clock_b);
        assert_eq!(clock_a.value(), 6); // max(5, 3) + 1

        let mut clock_c = LamportClock::with_value(2);
        let clock_d = LamportClock::with_value(10);

        clock_c.merge(&clock_d);
        assert_eq!(clock_c.value(), 11); // max(2, 10) + 1
    }

    #[test]
    fn test_merge_is_commutative_in_effect() {
        let clock_a = LamportClock::with_value(5);
        let clock_b = LamportClock::with_value(3);

        let mut result_ab = clock_a;
        result_ab.merge(&clock_b);

        let mut result_ba = clock_b;
        result_ba.merge(&clock_a);

        // Both should advance to 6 (max of 5,3 is 5, +1 = 6)
        assert_eq!(result_ab.value(), 6);
        assert_eq!(result_ba.value(), 6);
    }

    #[test]
    fn test_happened_before() {
        let clock_a = LamportClock::with_value(5);
        let clock_b = LamportClock::with_value(10);

        assert!(clock_a.happened_before(&clock_b));
        assert!(!clock_b.happened_before(&clock_a));
        assert!(!clock_a.happened_before(&clock_a)); // equal clocks
    }

    #[test]
    fn test_ordering() {
        let clock_a = LamportClock::with_value(1);
        let clock_b = LamportClock::with_value(2);
        let clock_c = LamportClock::with_value(2);

        assert!(clock_a < clock_b);
        assert!(clock_b > clock_a);
        assert!(clock_b == clock_c);
    }

    #[test]
    #[cfg(feature = "serde")]
    fn test_serde_roundtrip() {
        let clock = LamportClock::with_value(42);
        let serialized = serde_json::to_string(&clock).unwrap();
        let deserialized: LamportClock = serde_json::from_str(&serialized).unwrap();
        assert_eq!(clock, deserialized);
    }
}
