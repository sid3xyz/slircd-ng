//! Channel name utilities.
//!
//! This module provides utilities for working with IRC channel names.
//!
//! # Reference
//! - RFC 2812 Section 1.3: Channel names

/// Extension trait for checking if a string is a valid IRC channel name.
pub trait ChannelExt {
    /// Check if this string is a valid IRC channel name.
    ///
    /// Valid channel names:
    /// - Start with '#', '&', '+', or '!'
    /// - Do not contain space, comma, BEL (0x07), or NUL
    /// - Are at most 50 characters long
    fn is_channel_name(&self) -> bool;
}

impl ChannelExt for &str {
    fn is_channel_name(&self) -> bool {
        let mut chars = self.chars();

        // Must have a valid prefix
        let first = match chars.next() {
            Some(c) => c,
            None => return false,
        };

        match first {
            '#' | '&' | '+' | '!' => {}
            _ => return false,
        }

        // Length limit (RFC 2812 says 50 chars including prefix)
        if self.chars().count() > 50 {
            return false;
        }

        // Check for invalid characters
        for c in chars {
            if c == ' ' || c == ',' || c == '\x07' || c == '\0' || c.is_control() {
                return false;
            }
        }

        true
    }
}

impl ChannelExt for String {
    fn is_channel_name(&self) -> bool {
        self.as_str().is_channel_name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_channels() {
        assert!("#channel".is_channel_name());
        assert!("&local".is_channel_name());
        assert!("+modeless".is_channel_name());
        assert!("!safe12345".is_channel_name());
    }

    #[test]
    fn test_invalid_channels() {
        assert!(!"channel".is_channel_name()); // no prefix
        assert!(!"#chan nel".is_channel_name()); // space
        assert!(!"#chan,nel".is_channel_name()); // comma
        assert!(!"".is_channel_name()); // empty
    }
}
