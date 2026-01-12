//! IRC case-mapping functions.
//!
//! IRC uses a special case-insensitive comparison where some characters
//! are considered equivalent (e.g., `[` and `{`). This implements the
//! `rfc1459` case mapping which is the most common.

/// Convert a single character to IRC lowercase using RFC 1459 case mapping.
///
/// In addition to ASCII lowercase conversion, this maps:
/// - `[` → `{`
/// - `]` → `}`
/// - `\` → `|`
/// - `~` → `^`
#[inline]
pub const fn irc_lower_char(c: char) -> char {
    match c {
        '[' => '{',
        ']' => '}',
        '\\' => '|',
        '~' => '^',
        'A'..='Z' => (c as u8 + 32) as char,
        _ => c,
    }
}

/// Convert a string to IRC lowercase using RFC 1459 case mapping.
///
/// In addition to ASCII lowercase conversion, this maps:
/// - `[` → `{`
/// - `]` → `}`
/// - `\` → `|`
/// - `~` → `^`
pub fn irc_to_lower(s: &str) -> String {
    s.chars().map(irc_lower_char).collect()
}

/// Compare two strings using IRC case-insensitive comparison.
///
/// Uses the RFC 1459 case mapping where certain characters are equivalent.
pub fn irc_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.chars()
        .zip(b.chars())
        .all(|(ca, cb)| irc_lower_char(ca) == irc_lower_char(cb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_irc_lower_char() {
        // ASCII uppercase
        assert_eq!(irc_lower_char('A'), 'a');
        assert_eq!(irc_lower_char('Z'), 'z');
        assert_eq!(irc_lower_char('M'), 'm');

        // Special IRC chars
        assert_eq!(irc_lower_char('['), '{');
        assert_eq!(irc_lower_char(']'), '}');
        assert_eq!(irc_lower_char('\\'), '|');
        assert_eq!(irc_lower_char('~'), '^');

        // Already lowercase/other
        assert_eq!(irc_lower_char('a'), 'a');
        assert_eq!(irc_lower_char('0'), '0');
        assert_eq!(irc_lower_char('#'), '#');
    }

    #[test]
    fn test_irc_to_lower() {
        assert_eq!(irc_to_lower("HELLO"), "hello");
        assert_eq!(irc_to_lower("#Channel[1]"), "#channel{1}");
        assert_eq!(irc_to_lower("Nick\\Away"), "nick|away");
        assert_eq!(irc_to_lower("Test~Name"), "test^name");
    }

    #[test]
    fn test_irc_eq() {
        // Basic case insensitivity
        assert!(irc_eq("hello", "HELLO"));
        assert!(irc_eq("Hello", "hELLO"));

        // IRC special chars
        assert!(irc_eq("#channel[1]", "#CHANNEL{1}"));
        assert!(irc_eq("nick\\test", "NICK|TEST"));

        // Not equal
        assert!(!irc_eq("hello", "world"));
        assert!(!irc_eq("short", "longer"));
    }
}
