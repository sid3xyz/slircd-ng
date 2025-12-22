//! UID generation for TS6-style user identifiers.

use std::sync::atomic::{AtomicU64, Ordering};

/// Unique identifier for a user (TS6 UID string).
pub type Uid = String;

/// Generates unique user IDs (UIDs) in TS6 format.
///
/// Format: SID (3 chars) + Client ID (6 chars base36) = 9 chars total.
/// Example: "001AAAAAB"
///
/// Note: Counter starts at 2 because 0 (AAAAAA) and 1 (AAAAAB) are reserved
/// for service pseudoclients (NickServ, ChanServ).
pub struct UidGenerator {
    sid: String,
    counter: AtomicU64,
}

/// Start counter at 2 to skip reserved service UIDs (AAAAAA, AAAAAB).
const UID_COUNTER_START: u64 = 2;

impl UidGenerator {
    /// Create a new UID generator for the given server ID.
    pub fn new(sid: String) -> Self {
        Self {
            sid,
            counter: AtomicU64::new(UID_COUNTER_START),
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
        // First UID is AAAAAC because AAAAAA/AAAAAB are reserved for services
        assert_eq!(generator.next(), "001AAAAAC");
        assert_eq!(generator.next(), "001AAAAAD");
        assert_eq!(generator.next(), "001AAAAAE");
    }

    #[test]
    fn test_base36_encode() {
        assert_eq!(base36_encode_6(0), "AAAAAA");
        assert_eq!(base36_encode_6(1), "AAAAAB");
        assert_eq!(base36_encode_6(35), "AAAAA9");
        assert_eq!(base36_encode_6(36), "AAAABA");
    }
}
