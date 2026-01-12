//! IRC formatting control character utilities.
//!
//! This module defines which control characters are permitted in IRC message
//! content (formatting codes like bold, color, etc.) vs which are truly illegal
//! (NUL, BEL).
//!
//! # Background
//!
//! IRC messages use certain control characters for text formatting:
//! - CTCP delimiter (0x01)
//! - Bold (0x02)
//! - Color (0x03)
//! - Hex color (0x04)
//! - Reset (0x0F)
//! - Monospace (0x11)
//! - Reverse (0x16)
//! - Italic (0x1D)
//! - Strikethrough (0x1E)
//! - Underline (0x1F)
//!
//! These are defined by the IRC formatting specification at
//! <https://modern.ircdocs.horse/formatting>.
//!
//! Note: Formatting codes are permitted in message **content** but MUST NOT
//! appear in nicknames, usernames, or channel names per RFC 2812.

/// Returns true if the character is a valid IRC formatting code.
///
/// These characters are permitted in message content for text formatting.
///
/// # Examples
///
/// ```
/// use slirc_proto::format::is_irc_format_code;
///
/// assert!(is_irc_format_code('\x01')); // CTCP
/// assert!(is_irc_format_code('\x02')); // Bold
/// assert!(is_irc_format_code('\x03')); // Color
/// assert!(!is_irc_format_code('a'));   // Not a format code
/// assert!(!is_irc_format_code('\x00')); // NUL is not a format code
/// ```
#[inline]
pub fn is_irc_format_code(ch: char) -> bool {
    matches!(
        ch,
        '\x01' | '\x02' | '\x03' | '\x04' | '\x0F' | '\x11' | '\x16' | '\x1D' | '\x1E' | '\x1F'
    )
}

/// Returns true if a control character is illegal in IRC messages.
///
/// A character is illegal if it is:
/// - BEL (0x07) - always illegal in message content
/// - Any other control character that is NOT CR, LF, NUL, or a recognized format code
///
/// NUL (0x00) is allowed - Rust Strings handle binary data correctly and NUL
/// bytes don't cause issues in message content. They're only problematic in C strings.
/// This is necessary for commands like METADATA which may contain binary values.
///
/// CR (0x0D) and LF (0x0A) are permitted as line delimiters.
///
/// # Examples
///
/// ```
/// use slirc_proto::format::is_illegal_control_char;
///
/// // NUL is now allowed for binary data (e.g., METADATA)
/// assert!(!is_illegal_control_char('\x00')); // NUL - allowed
///
/// // BEL is always illegal
/// assert!(is_illegal_control_char('\x07')); // BEL
///
/// // Format codes are allowed
/// assert!(!is_illegal_control_char('\x01')); // CTCP
/// assert!(!is_illegal_control_char('\x02')); // Bold
/// assert!(!is_illegal_control_char('\x03')); // Color
///
/// // Line delimiters are allowed
/// assert!(!is_illegal_control_char('\r')); // CR
/// assert!(!is_illegal_control_char('\n')); // LF
///
/// // Normal characters are allowed
/// assert!(!is_illegal_control_char('a'));
/// assert!(!is_illegal_control_char(' '));
/// ```
#[inline]
pub fn is_illegal_control_char(ch: char) -> bool {
    // BEL is always illegal
    if ch == '\x07' {
        return true;
    }
    // Other control chars are illegal unless they are CR, LF, NUL, or a format code
    ch.is_control() && ch != '\r' && ch != '\n' && ch != '\0' && !is_irc_format_code(ch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_codes() {
        // All format codes should be recognized
        assert!(is_irc_format_code('\x01')); // CTCP
        assert!(is_irc_format_code('\x02')); // Bold
        assert!(is_irc_format_code('\x03')); // Color
        assert!(is_irc_format_code('\x04')); // Hex color
        assert!(is_irc_format_code('\x0F')); // Reset
        assert!(is_irc_format_code('\x11')); // Monospace
        assert!(is_irc_format_code('\x16')); // Reverse
        assert!(is_irc_format_code('\x1D')); // Italic
        assert!(is_irc_format_code('\x1E')); // Strikethrough
        assert!(is_irc_format_code('\x1F')); // Underline

        // Non-format codes
        assert!(!is_irc_format_code('a'));
        assert!(!is_irc_format_code('\x00')); // NUL
        assert!(!is_irc_format_code('\x07')); // BEL
        assert!(!is_irc_format_code('\r'));
        assert!(!is_irc_format_code('\n'));
    }

    #[test]
    fn test_illegal_control_chars() {
        // BEL is always illegal
        assert!(is_illegal_control_char('\x07'));

        // NUL is now allowed for binary data (e.g., METADATA values)
        assert!(!is_illegal_control_char('\x00'));

        // Format codes should NOT be illegal
        assert!(!is_illegal_control_char('\x01')); // CTCP
        assert!(!is_illegal_control_char('\x02')); // Bold
        assert!(!is_illegal_control_char('\x03')); // Color
        assert!(!is_illegal_control_char('\x1F')); // Underline

        // CR and LF are line delimiters, not illegal
        assert!(!is_illegal_control_char('\r'));
        assert!(!is_illegal_control_char('\n'));

        // Normal printable characters are fine
        assert!(!is_illegal_control_char('a'));
        assert!(!is_illegal_control_char(' '));
        assert!(!is_illegal_control_char('!'));

        // Other control chars that aren't format codes ARE illegal
        assert!(is_illegal_control_char('\x05')); // ENQ - not a format code
        assert!(is_illegal_control_char('\x06')); // ACK - not a format code
        assert!(is_illegal_control_char('\x08')); // BS - not a format code
    }

    #[test]
    fn test_ctcp_allowed() {
        // CTCP delimiter must be allowed for CTCP messages like ACTION
        assert!(!is_illegal_control_char('\x01'));
        assert!(is_irc_format_code('\x01'));
    }

    #[test]
    fn test_formatting_in_message() {
        // Simulate checking a formatted message
        let msg = "\x02bold\x02 and \x034,5colored\x03";
        for ch in msg.chars() {
            assert!(
                !is_illegal_control_char(ch),
                "Character {:?} should be allowed in message",
                ch
            );
        }
    }

    #[test]
    fn test_ctcp_action() {
        // CTCP ACTION message
        let msg = "\x01ACTION waves\x01";
        for ch in msg.chars() {
            assert!(
                !is_illegal_control_char(ch),
                "Character {:?} should be allowed in CTCP message",
                ch
            );
        }
    }
}
