//! Nickname validation utilities.
//!
//! This module provides utilities for validating IRC nicknames per RFC 2812.
//!
//! # Reference
//! - RFC 2812 Section 2.3.1: Message format (nickname definition)

/// Extension trait for checking if a string is a valid IRC nickname.
pub trait NickExt {
    /// Check if this string is a valid IRC nickname per RFC 2812.
    ///
    /// Valid nicknames:
    /// - First character: letter (a-z, A-Z) or special character `[\]^_`{|}`
    /// - Subsequent characters: letter, digit (0-9), special, or hyphen (-)
    /// - Maximum length: 30 characters (configurable per server via ISUPPORT NICKLEN)
    ///
    /// # Examples
    ///
    /// ```
    /// use slirc_proto::NickExt;
    ///
    /// assert!("nick".is_valid_nick());
    /// assert!("Nick123".is_valid_nick());
    /// assert!("[cool]".is_valid_nick());
    /// assert!("_under_".is_valid_nick());
    ///
    /// assert!(!"123nick".is_valid_nick());  // Can't start with digit
    /// assert!(!"".is_valid_nick());          // Empty
    /// assert!(!"nick name".is_valid_nick()); // Contains space
    /// ```
    fn is_valid_nick(&self) -> bool;

    /// Check if this string is a valid IRC nickname with a custom max length.
    ///
    /// Use this when you know the server's NICKLEN from ISUPPORT.
    fn is_valid_nick_len(&self, max_len: usize) -> bool;
}

/// Default maximum nickname length per RFC 2812.
pub const DEFAULT_NICK_MAX_LEN: usize = 30;

/// Check if a character is a "special" character allowed in nicknames.
///
/// Per RFC 2812: `[ ] \ ` ^ _ { | }`
#[inline]
fn is_special(c: char) -> bool {
    matches!(c, '[' | ']' | '\\' | '`' | '_' | '^' | '{' | '|' | '}')
}

impl NickExt for &str {
    fn is_valid_nick(&self) -> bool {
        self.is_valid_nick_len(DEFAULT_NICK_MAX_LEN)
    }

    fn is_valid_nick_len(&self, max_len: usize) -> bool {
        if self.is_empty() || self.len() > max_len {
            return false;
        }

        let mut chars = self.chars();

        // First character: letter or special
        let first = match chars.next() {
            Some(c) => c,
            None => return false,
        };

        if !first.is_ascii_alphabetic() && !is_special(first) {
            return false;
        }

        // Rest: letter, digit, special, or hyphen
        chars.all(|c| c.is_ascii_alphanumeric() || is_special(c) || c == '-')
    }
}

impl NickExt for String {
    fn is_valid_nick(&self) -> bool {
        self.as_str().is_valid_nick()
    }

    fn is_valid_nick_len(&self, max_len: usize) -> bool {
        self.as_str().is_valid_nick_len(max_len)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_nicks() {
        assert!("nick".is_valid_nick());
        assert!("Nick".is_valid_nick());
        assert!("nick123".is_valid_nick());
        assert!("NICK".is_valid_nick());
        assert!("n".is_valid_nick());
        assert!("nick-name".is_valid_nick());
    }

    #[test]
    fn test_special_chars() {
        assert!("[nick]".is_valid_nick());
        assert!("nick\\test".is_valid_nick());
        assert!("_nick_".is_valid_nick());
        assert!("^nick^".is_valid_nick());
        assert!("{nick}".is_valid_nick());
        assert!("|nick|".is_valid_nick());
        assert!("`nick`".is_valid_nick());
    }

    #[test]
    fn test_invalid_nicks() {
        assert!(!"".is_valid_nick()); // empty
        assert!(!"123nick".is_valid_nick()); // starts with digit
        assert!(!"nick name".is_valid_nick()); // space
        assert!(!"-nick".is_valid_nick()); // starts with hyphen
        assert!(!"nick@host".is_valid_nick()); // contains @
        assert!(!"nick!user".is_valid_nick()); // contains !
    }

    #[test]
    fn test_length_limits() {
        let long_nick = "a".repeat(31);
        assert!(!long_nick.as_str().is_valid_nick());

        let max_nick = "a".repeat(30);
        assert!(max_nick.as_str().is_valid_nick());

        // Custom length
        assert!("abcdef".is_valid_nick_len(5) == false);
        assert!("abcde".is_valid_nick_len(5) == true);
    }
}
