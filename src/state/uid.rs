//! UID generation for TS6-style user identifiers.

use std::sync::atomic::{AtomicU64, Ordering};

/// Generates unique user IDs (UIDs) in TS6 format.
///
/// Format: SID (3 chars) + Client ID (6 chars base36) = 9 chars total.
/// Example: "001AAAAAB"
pub struct UidGenerator {
    sid: String,
    counter: AtomicU64,
}

impl UidGenerator {
    /// Create a new UID generator for the given server ID.
    pub fn new(sid: String) -> Self {
        Self {
            sid,
            counter: AtomicU64::new(0),
        }
    }

    /// Generate the next unique UID.
    pub fn next(&self) -> String {
        let n = self.counter.fetch_add(1, Ordering::Relaxed);
        format!("{}{}", self.sid, base36_encode_6(n))
    }
}

/// Encode a number as a 6-character base36 string.
fn base36_encode_6(mut n: u64) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut result = [b'A'; 6];

    for i in (0..6).rev() {
        result[i] = CHARS[(n % 36) as usize];
        n /= 36;
    }

    String::from_utf8_lossy(&result).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uid_generation() {
        let generator = UidGenerator::new("001".to_string());
        assert_eq!(generator.next(), "001AAAAAA");
        assert_eq!(generator.next(), "001AAAAAB");
        assert_eq!(generator.next(), "001AAAAAC");
    }

    #[test]
    fn test_base36_encode() {
        assert_eq!(base36_encode_6(0), "AAAAAA");
        assert_eq!(base36_encode_6(1), "AAAAAB");
        assert_eq!(base36_encode_6(35), "AAAAA9");
        assert_eq!(base36_encode_6(36), "AAAABA");
    }
}
