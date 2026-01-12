//! IRC formatting code handling.
//!
//! This module provides utilities for detecting and stripping IRC formatting
//! codes (colors, bold, underline, etc.) from messages.
//!
//! # IRC Format Codes
//! - 0x02 (^B): Bold
//! - 0x03 (^C): Color (followed by optional foreground,background)
//! - 0x0F (^O): Reset all formatting
//! - 0x16 (^V): Reverse/Inverse
//! - 0x1F (^_): Underline

use std::borrow::Cow;

/// IRC format control characters.
const FORMAT_CHARS: &[char] = &[
    '\x02', // Bold
    '\x03', // Color
    '\x0F', // Reset
    '\x16', // Reverse
    '\x1F', // Underline
];

/// Extension trait for handling formatted IRC strings.
pub trait FormattedStringExt<'a> {
    /// Check if the string contains any IRC formatting codes.
    fn is_formatted(&self) -> bool;

    /// Strip all IRC formatting codes from the string.
    ///
    /// Returns `Cow::Borrowed` if no formatting was present,
    /// or `Cow::Owned` with the stripped string otherwise.
    fn strip_formatting(self) -> Cow<'a, str>;
}

impl<'a> FormattedStringExt<'a> for &'a str {
    fn is_formatted(&self) -> bool {
        self.contains(FORMAT_CHARS)
    }

    fn strip_formatting(self) -> Cow<'a, str> {
        if !self.is_formatted() {
            return Cow::Borrowed(self);
        }

        let mut result = String::with_capacity(self.len());
        let mut parser = ColorParser::new();

        for c in self.chars() {
            if parser.consume(c) {
                result.push(c);
            }
        }

        Cow::Owned(result)
    }
}

impl FormattedStringExt<'static> for String {
    fn is_formatted(&self) -> bool {
        self.as_str().is_formatted()
    }

    fn strip_formatting(mut self) -> Cow<'static, str> {
        if !self.is_formatted() {
            return Cow::Owned(self);
        }

        let mut parser = ColorParser::new();
        self.retain(|c| parser.consume(c));
        Cow::Owned(self)
    }
}

/// Parser state for stripping color codes.
enum State {
    /// Normal text
    Text,
    /// Just saw color code (0x03)
    ColorStart,
    /// Saw first digit of foreground
    Foreground1(char),
    /// Saw both digits of foreground
    Foreground2,
    /// Saw comma after foreground
    Comma,
    /// Saw first digit of background
    Background1(char),
}

struct ColorParser {
    state: State,
}

impl ColorParser {
    fn new() -> Self {
        Self { state: State::Text }
    }

    /// Consume a character, returning true if it should be kept.
    fn consume(&mut self, c: char) -> bool {
        use State::*;

        match self.state {
            // Start color sequence on ^C
            Text | Foreground1(_) | Foreground2 if c == '\x03' => {
                self.state = ColorStart;
                false
            }

            // Normal text - strip format chars, keep everything else
            Text => !FORMAT_CHARS.contains(&c),

            // After ^C, check for digits
            ColorStart if c.is_ascii_digit() => {
                self.state = Foreground1(c);
                false
            }

            // First digit seen - check for second digit or comma
            Foreground1('0') if c.is_ascii_digit() => {
                // 00-09
                self.state = Foreground2;
                false
            }
            Foreground1('1') if c.is_ascii_digit() && c < '6' => {
                // 10-15
                self.state = Foreground2;
                false
            }
            Foreground1(_) if c.is_ascii_digit() && c < '6' => {
                // Single-digit fg, this is next char
                self.state = Text;
                true
            }
            Foreground1(_) if c == ',' => {
                self.state = Comma;
                false
            }

            // After two fg digits, check for comma
            Foreground2 if c == ',' => {
                self.state = Comma;
                false
            }

            // After comma, check for background digits
            Comma if c.is_ascii_digit() => {
                self.state = Background1(c);
                false
            }

            // Background first digit - check for second
            Background1(prev) if c.is_ascii_digit() && c < '6' => {
                self.state = Text;
                // If prev was 1 and this completes 10-15, consume it
                prev != '1'
            }

            // Any other case - reset and process normally
            _ => {
                self.state = Text;
                !FORMAT_CHARS.contains(&c)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_formatted() {
        assert!("\x02bold\x02".is_formatted());
        assert!("\x034red\x03".is_formatted());
        assert!(!"plain text".is_formatted());
    }

    #[test]
    fn test_strip_basic() {
        assert_eq!("\x02bold\x02".strip_formatting(), "bold");
        assert_eq!("\x1Funderline".strip_formatting(), "underline");
    }

    #[test]
    fn test_strip_colors() {
        assert_eq!("\x034red".strip_formatting(), "red");
        assert_eq!("\x0304red".strip_formatting(), "red");
        assert_eq!("\x034,5colored".strip_formatting(), "colored");
    }

    #[test]
    fn test_no_formatting() {
        let s = "plain text";
        match s.strip_formatting() {
            Cow::Borrowed(b) => assert_eq!(b, "plain text"),
            Cow::Owned(_) => panic!("expected borrowed"),
        }
    }
}
