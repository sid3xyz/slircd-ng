//! IRCv3 message tag escaping utilities.

use std::fmt::{Result as FmtResult, Write};
use std::io::{self, Write as IoWrite};

/// Escape a tag value for serialization.
///
/// Escapes special characters according to the IRCv3 message-tags spec.
pub fn escape_tag_value(f: &mut dyn Write, value: &str) -> FmtResult {
    for c in value.chars() {
        match c {
            ';' => f.write_str("\\:")?,
            ' ' => f.write_str("\\s")?,
            '\\' => f.write_str("\\\\")?,
            '\r' => f.write_str("\\r")?,
            '\n' => f.write_str("\\n")?,
            c => f.write_char(c)?,
        }
    }
    Ok(())
}

/// Escape a tag value directly to an `io::Write` implementor.
///
/// This is the zero-copy version that avoids intermediate string allocation.
/// Used by the [`IrcEncode`](crate::encode::IrcEncode) trait.
pub fn escape_tag_value_to_writer<W: IoWrite>(w: &mut W, value: &str) -> io::Result<usize> {
    let mut written = 0;
    for c in value.chars() {
        written += match c {
            ';' => w.write(b"\\:")?,
            ' ' => w.write(b"\\s")?,
            '\\' => w.write(b"\\\\")?,
            '\r' => w.write(b"\\r")?,
            '\n' => w.write(b"\\n")?,
            c => {
                let mut buf = [0u8; 4];
                w.write(c.encode_utf8(&mut buf).as_bytes())?
            }
        };
    }
    Ok(written)
}

/// Unescape a tag value from wire format.
///
/// Reverses the escaping applied by [`escape_tag_value`].
pub fn unescape_tag_value(value: &str) -> String {
    let mut unescaped = String::with_capacity(value.len());
    let mut iter = value.chars();
    while let Some(c) = iter.next() {
        let r = if c == '\\' {
            match iter.next() {
                Some(':') => ';',
                Some('s') => ' ',
                Some('\\') => '\\',
                Some('r') => '\r',
                Some('n') => '\n',
                Some(c) => c,
                None => break,
            }
        } else {
            c
        };
        unescaped.push(r);
    }
    unescaped
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IRCv3 specifies these escape sequences:
    /// - `\:` → `;` (semicolon)
    /// - `\s` → ` ` (space)
    /// - `\\` → `\` (backslash)
    /// - `\r` → CR (carriage return)
    /// - `\n` → LF (line feed)
    #[test]
    fn test_unescape_semicolon() {
        assert_eq!(unescape_tag_value("a\\:b"), "a;b");
    }

    #[test]
    fn test_unescape_space() {
        assert_eq!(unescape_tag_value("hello\\sworld"), "hello world");
    }

    #[test]
    fn test_unescape_backslash() {
        assert_eq!(unescape_tag_value("path\\\\file"), "path\\file");
    }

    #[test]
    fn test_unescape_carriage_return() {
        assert_eq!(unescape_tag_value("line\\rend"), "line\rend");
    }

    #[test]
    fn test_unescape_line_feed() {
        assert_eq!(unescape_tag_value("line\\nend"), "line\nend");
    }

    #[test]
    fn test_unescape_combined() {
        // All escape sequences together
        let input = "a\\:b\\sc\\\\d\\re\\nf";
        let expected = "a;b c\\d\re\nf";
        assert_eq!(unescape_tag_value(input), expected);
    }

    #[test]
    fn test_unescape_trailing_backslash() {
        // Trailing backslash with no following char should be dropped per IRCv3
        assert_eq!(unescape_tag_value("test\\"), "test");
    }

    #[test]
    fn test_unescape_unknown_escape() {
        // Unknown escape sequences: \x becomes x (backslash dropped)
        assert_eq!(unescape_tag_value("a\\xb"), "axb");
    }

    #[test]
    fn test_escape_roundtrip() {
        let test_values = vec![
            "simple",
            "with space",
            "with;semicolon",
            "with\\backslash",
            "with\nnewline",
            "with\rcarriage",
            "complex; \\ \n \r all",
        ];

        for original in test_values {
            let mut escaped = String::new();
            escape_tag_value(&mut escaped, original).unwrap();
            let unescaped = unescape_tag_value(&escaped);
            assert_eq!(
                unescaped, original,
                "Roundtrip failed: '{}' -> '{}' -> '{}'",
                original, escaped, unescaped
            );
        }
    }
}
