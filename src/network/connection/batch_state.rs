//! Batch state management utilities.
//!
//! Provides helper functions for generating batch identifiers and managing
//! batch-related state.

/// Convert a u32 to a base36 string (lowercase).
///
/// Used for generating compact batch IDs from monotonic counters.
pub(super) fn to_base36(mut value: u32) -> String {
    if value == 0 {
        return "0".to_string();
    }

    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut buf = Vec::with_capacity(7);
    while value > 0 {
        let rem = (value % 36) as usize;
        buf.push(DIGITS[rem]);
        value /= 36;
    }
    buf.reverse();
    String::from_utf8(buf).unwrap_or_default()
}
