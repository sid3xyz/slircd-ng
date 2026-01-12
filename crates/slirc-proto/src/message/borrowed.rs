//! Zero-copy borrowed message types for high-performance parsing.
//!
//! This module provides `MessageRef<'a>`, a borrowed message type that holds
//! references to the original input string, avoiding allocations during parsing.
//!
//! # Example
//!
//! ```
//! use slirc_proto::message::MessageRef;
//!
//! let raw = "@time=2023-01-01T00:00:00Z :nick!user@host PRIVMSG #channel :Hello!";
//! let msg = MessageRef::parse(raw).unwrap();
//!
//! assert_eq!(msg.command_name(), "PRIVMSG");
//! assert_eq!(msg.tag_value("time"), Some("2023-01-01T00:00:00Z"));
//! assert_eq!(msg.source_nickname(), Some("nick"));
//! ```

use std::fmt::{self, Display, Formatter};

use crate::command::Command;
use crate::command::CommandRef;
use crate::error::MessageParseError;
use crate::message::{Message, Tag};
use crate::prefix::PrefixRef;

use super::nom_parser::ParsedMessage;

/// A borrowed IRC message that references the original input string.
///
/// This is the zero-copy alternative to [`Message`]. All string data is borrowed
/// from the original input, making parsing very fast with no allocations.
///
/// Use this when:
/// - Processing many messages in a tight loop
/// - You only need to inspect message contents briefly
/// - Memory/allocation overhead is a concern
///
/// Use [`Message`] when:
/// - You need to store the message for later use
/// - You need to modify the message
/// - The original input string won't outlive the message
#[derive(Clone, PartialEq, Debug)]
pub struct MessageRef<'a> {
    /// Raw IRCv3 tags string (without the leading `@`), if present.
    pub tags: Option<&'a str>,
    /// Parsed message prefix, if present.
    pub prefix: Option<PrefixRef<'a>>,
    /// The command with its arguments.
    pub command: CommandRef<'a>,
    /// The original raw message string.
    pub raw: &'a str,
}

impl<'a> MessageRef<'a> {
    /// Parse an IRC message string into a borrowed `MessageRef`.
    ///
    /// This is the primary entry point for zero-copy parsing. The returned
    /// `MessageRef` borrows from the input string, so the input must outlive
    /// the returned value.
    ///
    /// # Example
    ///
    /// ```
    /// use slirc_proto::message::MessageRef;
    ///
    /// let msg = MessageRef::parse(":server PING :timestamp").unwrap();
    /// assert_eq!(msg.command_name(), "PING");
    /// ```
    #[must_use = "parsing result should be handled"]
    pub fn parse(s: &'a str) -> Result<MessageRef<'a>, MessageParseError> {
        if s.is_empty() {
            return Err(MessageParseError::EmptyMessage);
        }

        let trimmed = s.trim_end_matches(['\r', '\n']);

        let parsed = match ParsedMessage::parse(trimmed) {
            Ok(m) => m,
            Err(_e) => return Err(MessageParseError::InvalidCommand),
        };

        let prefix = parsed.prefix.map(PrefixRef::parse);
        let command = CommandRef::new(parsed.command, parsed.params.clone());

        Ok(MessageRef {
            tags: parsed.tags,
            prefix,
            command,
            raw: s,
        })
    }

    /// Get the command name.
    #[inline]
    pub fn command_name(&self) -> &str {
        self.command.name
    }

    /// Get the command arguments.
    #[inline]
    pub fn args(&self) -> &[&'a str] {
        &self.command.args
    }

    /// Get a specific argument by index.
    #[inline]
    pub fn arg(&self, index: usize) -> Option<&'a str> {
        self.command.args.get(index).copied()
    }

    /// Get the value of a tag by key.
    ///
    /// Tags are stored as a raw string in the format `key1=value1;key2=value2`.
    /// This method parses the tags string on-demand to find the requested key.
    ///
    /// # Example
    ///
    /// ```
    /// use slirc_proto::message::MessageRef;
    ///
    /// let msg = MessageRef::parse("@time=2023-01-01;msgid=abc123 PING").unwrap();
    /// assert_eq!(msg.tag_value("time"), Some("2023-01-01"));
    /// assert_eq!(msg.tag_value("msgid"), Some("abc123"));
    /// assert_eq!(msg.tag_value("missing"), None);
    /// ```
    pub fn tag_value(&self, key: &str) -> Option<&'a str> {
        let tags = self.tags?;
        for tag in tags.split(';') {
            if let Some((k, v)) = tag.split_once('=') {
                if k == key {
                    return Some(v);
                }
            } else if tag == key {
                // Tag with no value
                return Some("");
            }
        }
        None
    }

    /// Check if a tag exists (regardless of value).
    pub fn has_tag(&self, key: &str) -> bool {
        let Some(tags) = self.tags else { return false };
        for tag in tags.split(';') {
            let k = tag.split_once('=').map(|(k, _)| k).unwrap_or(tag);
            if k == key {
                return true;
            }
        }
        false
    }

    /// Iterate over all tags as (key, value) pairs.
    ///
    /// Tags without values will have an empty string as the value.
    pub fn tags_iter(&self) -> impl Iterator<Item = (&'a str, &'a str)> {
        self.tags
            .into_iter()
            .flat_map(|tags| tags.split(';'))
            .map(|tag| tag.split_once('=').unwrap_or((tag, "")))
    }

    /// Get the server time tag value, if present.
    #[inline]
    pub fn server_time(&self) -> Option<&'a str> {
        self.tag_value("time")
    }

    /// Get the msgid tag value, if present.
    #[inline]
    pub fn msgid(&self) -> Option<&'a str> {
        self.tag_value("msgid")
    }

    /// Get the label tag value, if present.
    #[inline]
    pub fn label(&self) -> Option<&'a str> {
        self.tag_value("label")
    }

    /// Get the account tag value, if present.
    #[inline]
    pub fn account_tag(&self) -> Option<&'a str> {
        self.tag_value("account")
    }

    /// Get the source nickname from the prefix, if present.
    ///
    /// Returns `None` if there's no prefix or if the prefix is a server name.
    pub fn source_nickname(&self) -> Option<&'a str> {
        self.prefix.as_ref().and_then(|p| p.nickname())
    }

    /// Get the source username from the prefix, if present.
    pub fn source_user(&self) -> Option<&'a str> {
        self.prefix.as_ref().and_then(|p| p.user)
    }

    /// Get the source host from the prefix, if present.
    pub fn source_host(&self) -> Option<&'a str> {
        self.prefix.as_ref().and_then(|p| p.host)
    }

    /// Get the raw prefix string, if present.
    pub fn raw_prefix(&self) -> Option<&'a str> {
        self.prefix.as_ref().map(|p| p.raw)
    }

    /// Convert this borrowed message to an owned [`Message`].
    ///
    /// This allocates new strings for all components. Use this when you need
    /// to store the message beyond the lifetime of the input string.
    pub fn to_owned(&self) -> Message {
        let tags = self.tags.map(|raw_tags| {
            raw_tags
                .split(';')
                .map(|tag| {
                    if let Some((k, v)) = tag.split_once('=') {
                        Tag::new(k, Some(super::tags::unescape_tag_value(v)))
                    } else {
                        Tag::new(tag, None)
                    }
                })
                .collect()
        });

        let prefix = self.prefix.as_ref().map(|p| p.to_owned());
        let command = self.to_owned_command();

        Message {
            tags,
            prefix,
            command,
        }
    }

    /// Convert the command to an owned Command.
    fn to_owned_command(&self) -> Command {
        Command::new(self.command.name, self.command.args.to_vec()).unwrap_or_else(|_| {
            Command::Raw(
                self.command.name.to_string(),
                self.command.args.iter().map(|s| s.to_string()).collect(),
            )
        })
    }

    /// Serialize back to a raw IRC message string (without trailing CRLF).
    ///
    /// Note: Unlike [`Display`], this does not append `\r\n` to match the
    /// original input format.
    pub fn to_raw_owned(&self) -> String {
        let capacity = self.raw.len();
        let mut s = String::with_capacity(capacity);

        if let Some(tags) = &self.tags {
            s.push('@');
            s.push_str(tags);
            s.push(' ');
        }
        if let Some(prefix) = &self.prefix {
            s.push(':');
            s.push_str(prefix.raw);
            s.push(' ');
        }
        s.push_str(self.command.name);
        for (i, arg) in self.command.args.iter().enumerate() {
            s.push(' ');
            // Last argument needs colon prefix if it:
            // - contains a space
            // - is empty
            // - starts with ':' (to distinguish from prefix marker)
            let is_last = i == self.command.args.len() - 1;
            let needs_colon =
                is_last && (arg.contains(' ') || arg.is_empty() || arg.starts_with(':'));
            if needs_colon {
                s.push(':');
            }
            s.push_str(arg);
        }
        s
    }

    /// Check if this is a PRIVMSG command.
    #[inline]
    pub fn is_privmsg(&self) -> bool {
        self.command.name.eq_ignore_ascii_case("PRIVMSG")
    }

    /// Check if this is a NOTICE command.
    #[inline]
    pub fn is_notice(&self) -> bool {
        self.command.name.eq_ignore_ascii_case("NOTICE")
    }

    /// Check if this is a numeric response (3-digit command).
    #[inline]
    pub fn is_numeric(&self) -> bool {
        self.command.name.len() == 3 && self.command.name.chars().all(|c| c.is_ascii_digit())
    }

    /// Get the numeric response code if this is a numeric response.
    pub fn numeric_code(&self) -> Option<u16> {
        if self.is_numeric() {
            self.command.name.parse().ok()
        } else {
            None
        }
    }
}

impl Display for MessageRef<'_> {
    /// Serialize the message to IRC wire format.
    ///
    /// The output includes the trailing `\r\n` for consistency with [`Message`]'s
    /// Display implementation.
    ///
    /// # Example
    ///
    /// ```
    /// use slirc_proto::message::MessageRef;
    ///
    /// let msg = MessageRef::parse(":nick PRIVMSG #channel :Hello").unwrap();
    /// assert_eq!(format!("{}", msg), ":nick PRIVMSG #channel Hello\r\n");
    /// ```
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(tags) = &self.tags {
            write!(f, "@{} ", tags)?;
        }
        if let Some(prefix) = &self.prefix {
            write!(f, ":{} ", prefix.raw)?;
        }
        write!(f, "{}", self.command.name)?;
        for (i, arg) in self.command.args.iter().enumerate() {
            // Last argument needs colon prefix if it:
            // - contains a space
            // - is empty
            // - starts with ':' (to distinguish from prefix marker)
            let is_last = i == self.command.args.len() - 1;
            let needs_colon =
                is_last && (arg.contains(' ') || arg.is_empty() || arg.starts_with(':'));
            if needs_colon {
                write!(f, " :{}", arg)?;
            } else {
                write!(f, " {}", arg)?;
            }
        }
        write!(f, "\r\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        let msg = MessageRef::parse("PING :server").unwrap();
        assert_eq!(msg.command_name(), "PING");
        assert_eq!(msg.args(), &["server"]);
        assert!(msg.prefix.is_none());
        assert!(msg.tags.is_none());
    }

    #[test]
    fn test_parse_with_prefix() {
        let msg = MessageRef::parse(":nick!user@host PRIVMSG #channel :Hello").unwrap();
        assert_eq!(msg.command_name(), "PRIVMSG");
        assert_eq!(msg.source_nickname(), Some("nick"));
        assert_eq!(msg.source_user(), Some("user"));
        assert_eq!(msg.source_host(), Some("host"));
        assert_eq!(msg.args(), &["#channel", "Hello"]);
    }

    #[test]
    fn test_parse_with_tags() {
        let msg = MessageRef::parse("@time=2023-01-01;msgid=abc PING").unwrap();
        assert_eq!(msg.tag_value("time"), Some("2023-01-01"));
        assert_eq!(msg.tag_value("msgid"), Some("abc"));
        assert_eq!(msg.tag_value("missing"), None);
        assert!(msg.has_tag("time"));
        assert!(!msg.has_tag("missing"));
    }

    #[test]
    fn test_tags_iter() {
        let msg = MessageRef::parse("@a=1;b=2;c PING").unwrap();
        let tags: Vec<_> = msg.tags_iter().collect();
        assert_eq!(tags, vec![("a", "1"), ("b", "2"), ("c", "")]);
    }

    #[test]
    fn test_to_owned() {
        let msg = MessageRef::parse("@time=2023 :nick!user@host PRIVMSG #ch :Hi").unwrap();
        let owned = msg.to_owned();
        assert!(owned.tags.is_some());
        assert!(owned.prefix.is_some());
    }

    #[test]
    fn test_is_numeric() {
        let msg = MessageRef::parse(":server 001 nick :Welcome").unwrap();
        assert!(msg.is_numeric());
        assert_eq!(msg.numeric_code(), Some(1));

        let msg = MessageRef::parse("PING :server").unwrap();
        assert!(!msg.is_numeric());
        assert_eq!(msg.numeric_code(), None);
    }

    #[test]
    fn test_to_raw_owned() {
        let raw = ":nick PRIVMSG #channel :Hello world";
        let msg = MessageRef::parse(raw).unwrap();
        assert_eq!(msg.to_raw_owned(), raw);
    }

    #[test]
    fn test_display() {
        // Simple command - no space in arg, colon not needed
        let msg = MessageRef::parse("PING :server").unwrap();
        assert_eq!(format!("{msg}"), "PING server\r\n");

        // With prefix and trailing arg containing spaces
        let msg = MessageRef::parse(":nick PRIVMSG #channel :Hello world").unwrap();
        assert_eq!(format!("{msg}"), ":nick PRIVMSG #channel :Hello world\r\n");

        // With tags - single word arg without special chars, no colon needed
        let msg = MessageRef::parse("@time=2023;msgid=abc :nick NOTICE #ch :Hi").unwrap();
        assert_eq!(
            format!("{msg}"),
            "@time=2023;msgid=abc :nick NOTICE #ch Hi\r\n"
        );

        // Empty trailing arg - needs colon
        let msg = MessageRef::parse(":nick QUIT :").unwrap();
        assert_eq!(format!("{msg}"), ":nick QUIT :\r\n");

        // Arg with spaces - needs colon
        let msg = MessageRef::parse("PRIVMSG #test :hello there").unwrap();
        assert_eq!(format!("{msg}"), "PRIVMSG #test :hello there\r\n");

        // Arg starting with colon - needs colon prefix (per IRC protocol)
        let msg = MessageRef::parse("PRIVMSG #test ::)").unwrap();
        assert_eq!(format!("{msg}"), "PRIVMSG #test ::)\r\n");
    }
}
