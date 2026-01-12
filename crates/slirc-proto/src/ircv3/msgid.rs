//! Message ID generation for IRCv3 message-ids capability.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static MSGID_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique message ID string.
///
/// Returns a string like `1234567890-0` combining timestamp and counter.
pub fn generate_msgid() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let counter = MSGID_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", timestamp, counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_msgid_format() {
        let id = generate_msgid();
        // Should contain a hyphen
        assert!(id.contains('-'));
        // Should have timestamp-counter format
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 2);
        // Both parts should be numeric
        assert!(parts[0].parse::<u128>().is_ok());
        assert!(parts[1].parse::<u64>().is_ok());
    }

    #[test]
    fn test_msgid_uniqueness() {
        let id1 = generate_msgid();
        let id2 = generate_msgid();
        let id3 = generate_msgid();
        // All should be unique
        assert_ne!(id1, id2);
        assert_ne!(id2, id3);
        assert_ne!(id1, id3);
    }

    #[test]
    fn test_msgid_counter_increments() {
        let id1 = generate_msgid();
        let id2 = generate_msgid();
        let counter1: u64 = id1.split('-').nth(1).unwrap().parse().unwrap();
        let counter2: u64 = id2.split('-').nth(1).unwrap().parse().unwrap();
        // Counter should increment
        assert!(counter2 > counter1);
    }
}
