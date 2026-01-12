//! Nom-based IRC message parser.
//!
//! This module provides zero-copy parsing of IRC messages using the nom
//! parser combinator library.

use nom::{
    bytes::complete::{take_until, take_while1},
    character::complete::{char, space0},
    combinator::opt,
    error::ErrorKind,
    sequence::preceded,
    IResult,
};
use smallvec::SmallVec;

/// Parse IRCv3 message tags (the part after `@` and before the first space).
fn parse_tags(input: &str) -> IResult<&str, &str> {
    preceded(char('@'), take_until(" "))(input)
}

/// Parse message prefix (the part after `:` and before the first space).
fn parse_prefix(input: &str) -> IResult<&str, &str> {
    preceded(char(':'), take_while1(|c| c != ' '))(input)
}

/// Parse the command name (1*letter or 3digit).
fn parse_command(input: &str) -> IResult<&str, &str> {
    let (rest, cmd) = take_while1(|c: char| c.is_alphanumeric())(input)?;

    // RFC 2812: command = 1*letter / 3digit
    let is_all_letters = cmd.chars().all(|c| c.is_ascii_alphabetic());
    let is_three_digits = cmd.len() == 3 && cmd.chars().all(|c| c.is_ascii_digit());

    if is_all_letters || is_three_digits {
        Ok((rest, cmd))
    } else {
        Err(nom::Err::Error(nom::error::Error::new(
            input,
            ErrorKind::AlphaNumeric,
        )))
    }
}

/// Parse IRC message parameters from the remaining input after the command.
///
/// Handles both regular space-separated parameters and the trailing parameter
/// (prefixed with `:`) which may contain spaces. Multiple consecutive spaces
/// are treated as a single separator (RFC compliance).
///
/// Enforces the RFC 2812 limit of 15 parameters.
fn parse_params(input: &str) -> (&str, SmallVec<[&str; 15]>) {
    let mut params: SmallVec<[&str; 15]> = SmallVec::new();
    let mut rest = input;

    while let Some(b' ') = rest.as_bytes().first().copied() {
        // RFC 2812: at most 15 parameters
        if params.len() >= 15 {
            break;
        }

        // Skip all leading spaces (handles multiple consecutive spaces)
        while rest.as_bytes().first() == Some(&b' ') {
            rest = &rest[1..];
        }

        // Check if we've reached the end after skipping spaces
        if rest.is_empty() || rest.starts_with('\r') || rest.starts_with('\n') {
            break;
        }

        if let Some(b':') = rest.as_bytes().first().copied() {
            // Trailing parameter - everything after `:` until line end
            let after_colon = &rest[1..];
            let end = after_colon.find(['\r', '\n']).unwrap_or(after_colon.len());
            params.push(&after_colon[..end]);
            rest = &after_colon[end..];
            break;
        }

        // Regular parameter - until next space or line end
        let end = rest.find([' ', '\r', '\n']).unwrap_or(rest.len());
        let param = &rest[..end];
        if param.is_empty() {
            break;
        }
        params.push(param);
        rest = &rest[end..];
    }

    (rest, params)
}

/// Parse a complete IRC message into its components.
///
/// IRC message format:
/// ```text
/// [@tags] [:prefix] <command> [params...] [:trailing]
/// ```
pub(crate) fn parse_message(input: &str) -> IResult<&str, ParsedMessage<'_>> {
    // Parse optional tags
    let (input, tags) = opt(parse_tags)(input)?;
    let (input, _) = space0(input)?;

    // Parse optional prefix
    let (input, prefix) = opt(parse_prefix)(input)?;
    let (input, _) = space0(input)?;

    // Parse command (required)
    let (input, command) = parse_command(input)?;

    // Parse parameters (including trailing)
    let (rest, params) = parse_params(input);

    Ok((
        rest,
        ParsedMessage {
            tags,
            prefix,
            command,
            params,
        },
    ))
}

/// A parsed IRC message with borrowed string slices.
///
/// This is the intermediate representation produced by the nom parser.
/// It holds references into the original input string for zero-copy parsing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedMessage<'a> {
    /// Raw tags string (without the leading `@`), if present.
    pub tags: Option<&'a str>,
    /// Raw prefix string (without the leading `:`), if present.
    pub prefix: Option<&'a str>,
    /// The command name.
    pub command: &'a str,
    /// Command parameters, including trailing.
    pub params: SmallVec<[&'a str; 15]>,
}

impl<'a> ParsedMessage<'a> {
    /// Parse an IRC message string into a `ParsedMessage`.
    ///
    /// This is the primary entry point for parsing borrowed messages.
    /// Returns detailed error information for debugging failed parses.
    pub fn parse(input: &'a str) -> Result<Self, DetailedParseError> {
        match parse_message(input) {
            Ok((_remaining, msg)) => Ok(msg),
            Err(nom::Err::Error(e)) | Err(nom::Err::Failure(e)) => {
                let position = input.len() - e.input.len();
                Err(DetailedParseError {
                    input: input.to_string(),
                    position,
                    kind: e.code,
                })
            }
            Err(nom::Err::Incomplete(_)) => Err(DetailedParseError {
                input: input.to_string(),
                position: input.len(),
                kind: ErrorKind::Eof,
            }),
        }
    }
}

/// Detailed parse error with position information.
#[derive(Debug, Clone)]
pub(crate) struct DetailedParseError {
    /// The original input string that failed to parse.
    pub input: String,
    /// Character position where parsing failed.
    pub position: usize,
    /// The nom error kind.
    pub kind: ErrorKind,
}

impl std::fmt::Display for DetailedParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Parse error at position {}: {:?}",
            self.position, self.kind
        )?;

        // Show the error position in the input
        if self.position < self.input.len() {
            let before = &self.input[..self.position];
            let after = &self.input[self.position..];
            write!(f, "\n  Input: {}<<<HERE>>>{}", before, after)?;
        } else {
            write!(f, "\n  Input: {}<<<EOF>>>", self.input)?;
        }

        Ok(())
    }
}

impl std::error::Error for DetailedParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_command() {
        let msg = ParsedMessage::parse("PING").unwrap();
        assert_eq!(msg.command, "PING");
        assert!(msg.tags.is_none());
        assert!(msg.prefix.is_none());
        assert!(msg.params.is_empty());
    }

    #[test]
    fn test_parse_command_with_params() {
        let msg = ParsedMessage::parse("PRIVMSG #channel :Hello, world!").unwrap();
        assert_eq!(msg.command, "PRIVMSG");
        assert_eq!(msg.params.as_slice(), &["#channel", "Hello, world!"]);
    }

    #[test]
    fn test_parse_with_prefix() {
        let msg = ParsedMessage::parse(":nick!user@host PRIVMSG #channel :Hello").unwrap();
        assert_eq!(msg.prefix, Some("nick!user@host"));
        assert_eq!(msg.command, "PRIVMSG");
        assert_eq!(msg.params.as_slice(), &["#channel", "Hello"]);
    }

    #[test]
    fn test_parse_with_tags() {
        let msg = ParsedMessage::parse("@time=2023-01-01T00:00:00Z :nick PRIVMSG #ch :Hi").unwrap();
        assert_eq!(msg.tags, Some("time=2023-01-01T00:00:00Z"));
        assert_eq!(msg.prefix, Some("nick"));
        assert_eq!(msg.command, "PRIVMSG");
        assert_eq!(msg.params.as_slice(), &["#ch", "Hi"]);
    }

    #[test]
    fn test_parse_with_crlf() {
        let msg = ParsedMessage::parse("PING :server\r\n").unwrap();
        assert_eq!(msg.command, "PING");
        assert_eq!(msg.params.as_slice(), &["server"]);
    }

    #[test]
    fn test_parse_multiple_params() {
        let msg = ParsedMessage::parse("USER guest 0 * :Real Name").unwrap();
        assert_eq!(msg.command, "USER");
        assert_eq!(msg.params.as_slice(), &["guest", "0", "*", "Real Name"]);
    }

    #[test]
    fn test_parse_numeric_response() {
        let msg = ParsedMessage::parse(":server 001 nick :Welcome").unwrap();
        assert_eq!(msg.prefix, Some("server"));
        assert_eq!(msg.command, "001");
        assert_eq!(msg.params.as_slice(), &["nick", "Welcome"]);
    }

    #[test]
    fn test_parse_join() {
        let msg = ParsedMessage::parse(":nick!user@host JOIN #channel").unwrap();
        assert_eq!(msg.command, "JOIN");
        assert_eq!(msg.params.as_slice(), &["#channel"]);
    }

    #[test]
    fn test_parse_empty_trailing() {
        let msg = ParsedMessage::parse("PRIVMSG #channel :").unwrap();
        assert_eq!(msg.params.as_slice(), &["#channel", ""]);
    }

    #[test]
    fn test_parse_complex_tags() {
        let msg =
            ParsedMessage::parse("@msgid=abc123;time=2023-01-01 :nick PRIVMSG #ch :msg").unwrap();
        assert_eq!(msg.tags, Some("msgid=abc123;time=2023-01-01"));
    }

    #[test]
    fn test_parse_command_validation() {
        // Valid commands
        assert!(ParsedMessage::parse("PING").is_ok());
        assert!(ParsedMessage::parse("123").is_ok());

        // Invalid commands
        assert!(ParsedMessage::parse("PING123").is_err());
        assert!(ParsedMessage::parse("12").is_err());
        assert!(ParsedMessage::parse("1234").is_err());
    }

    #[test]
    fn test_parse_params_limit() {
        // 15 parameters (14 middle + 1 trailing)
        let raw = "CMD p1 p2 p3 p4 p5 p6 p7 p8 p9 p10 p11 p12 p13 p14 :p15";
        let msg = ParsedMessage::parse(raw).unwrap();
        assert_eq!(msg.params.len(), 15);

        // 16 parameters - should be truncated to 15
        let raw = "CMD p1 p2 p3 p4 p5 p6 p7 p8 p9 p10 p11 p12 p13 p14 p15 p16";
        let msg = ParsedMessage::parse(raw).unwrap();
        assert_eq!(msg.params.len(), 15);
        assert_eq!(msg.params[14], "p15");
    }

    #[test]
    fn test_parse_who_with_whox_fields() {
        // This is the exact command irctest sends for WHOX
        let msg = ParsedMessage::parse("WHO coolNick %r").unwrap();
        assert_eq!(msg.command, "WHO");
        assert_eq!(
            msg.params.len(),
            2,
            "Expected 2 params, got {:?}",
            msg.params
        );
        assert_eq!(msg.params[0], "coolNick");
        assert_eq!(msg.params[1], "%r");
    }

    #[test]
    fn test_parse_who_with_whox_token() {
        let msg = ParsedMessage::parse("WHO coolNick %cuhnar,123").unwrap();
        assert_eq!(msg.command, "WHO");
        assert_eq!(msg.params.len(), 2);
        assert_eq!(msg.params[0], "coolNick");
        assert_eq!(msg.params[1], "%cuhnar,123");
    }

    #[test]
    fn test_parse_mode_with_space_trailing() {
        // MODE #chan +k : (trailing parameter with just a space)
        let msg = ParsedMessage::parse("MODE #chan +k : ").unwrap();
        assert_eq!(msg.command, "MODE");
        assert_eq!(msg.params.len(), 3, "Expected 3 params: {:?}", msg.params);
        assert_eq!(msg.params[0], "#chan");
        assert_eq!(msg.params[1], "+k");
        assert_eq!(msg.params[2], " ", "Trailing should be a single space");
    }

    #[test]
    fn test_parse_mode_with_empty_trailing() {
        // MODE #chan +k : (trailing parameter that's empty)
        let msg = ParsedMessage::parse("MODE #chan +k :").unwrap();
        assert_eq!(msg.command, "MODE");
        assert_eq!(msg.params.len(), 3, "Expected 3 params: {:?}", msg.params);
        assert_eq!(msg.params[0], "#chan");
        assert_eq!(msg.params[1], "+k");
        assert_eq!(msg.params[2], "", "Trailing should be empty string");
    }
}
