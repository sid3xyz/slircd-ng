//! Batch reference generation for IRCv3 BATCH command.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static BATCH_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a unique batch reference string.
///
/// Returns a string like `1234567890-0` combining timestamp and counter.
pub fn generate_batch_ref() -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let counter = BATCH_COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{}-{}", timestamp, counter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_batch_ref_format() {
        let ref_id = generate_batch_ref();
        // Should contain a hyphen
        assert!(ref_id.contains('-'));
        // Should have timestamp-counter format
        let parts: Vec<&str> = ref_id.split('-').collect();
        assert_eq!(parts.len(), 2);
    }

    #[test]
    fn test_batch_ref_uniqueness() {
        let ref1 = generate_batch_ref();
        let ref2 = generate_batch_ref();
        let ref3 = generate_batch_ref();
        // All should be unique
        assert_ne!(ref1, ref2);
        assert_ne!(ref2, ref3);
    }

    #[test]
    fn test_batch_ref_counter_increments() {
        let ref1 = generate_batch_ref();
        let ref2 = generate_batch_ref();
        let counter1: u64 = ref1.split('-').nth(1).unwrap().parse().unwrap();
        let counter2: u64 = ref2.split('-').nth(1).unwrap().parse().unwrap();
        assert!(counter2 > counter1);
    }
}
