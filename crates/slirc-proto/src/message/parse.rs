//! Message parsing implementation.
//!
//! This module implements `FromStr` for `Message` using the nom-based parser.

use std::borrow::Cow;
use std::str::FromStr;

use crate::error::{MessageParseError, ProtocolError};

use super::nom_parser::ParsedMessage;
use super::tags::unescape_tag_value;
use super::types::{Message, Tag};

/// Intern common tag keys to avoid allocations.
///
/// IRCv3 messages frequently use the same tag keys. By returning
/// `Cow::Borrowed` for known keys, we avoid heap allocations.
#[inline]
fn intern_tag_key(key: &str) -> Cow<'static, str> {
    match key {
        // Core IRCv3 tags
        "msgid" => Cow::Borrowed("msgid"),
        "time" => Cow::Borrowed("time"),
        "batch" => Cow::Borrowed("batch"),
        "account" => Cow::Borrowed("account"),
        "label" => Cow::Borrowed("label"),

        // Capability tags
        "echo-message" => Cow::Borrowed("echo-message"),
        "message-tags" => Cow::Borrowed("message-tags"),

        // Typing indicators
        "+typing" => Cow::Borrowed("+typing"),
        "+draft/typing" => Cow::Borrowed("+draft/typing"),

        // Reply tags
        "+draft/reply" => Cow::Borrowed("+draft/reply"),
        "+draft/react" => Cow::Borrowed("+draft/react"),

        // Other common tags
        _ => Cow::Owned(key.to_owned()),
    }
}

/// Parse a raw tags string into a vector of `Tag` structs.
///
/// The input should be the tags portion without the leading `@`.
fn parse_tags_string(tags_str: &str) -> Vec<Tag> {
    tags_str
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|tag| {
            let mut iter = tag.splitn(2, '=');
            let key = iter.next().unwrap_or("");
            let value = iter.next().map(unescape_tag_value);

            let interned_key = if key.is_empty() {
                Cow::Owned(String::new())
            } else {
                intern_tag_key(key)
            };

            Tag(interned_key, value)
        })
        .collect()
}

impl FromStr for Message {
    type Err = ProtocolError;

    fn from_str(s: &str) -> Result<Message, Self::Err> {
        if s.is_empty() {
            return Err(ProtocolError::InvalidMessage {
                string: s.to_owned(),
                cause: MessageParseError::EmptyMessage,
            });
        }

        // Use the nom parser
        let parsed = ParsedMessage::parse(s).map_err(|parse_err| {
            // Convert detailed parse error to appropriate message parse error
            let cause = MessageParseError::ParseContext {
                position: parse_err.position,
                context: format!("Parse error: {:?}", parse_err.kind),
                source: None,
                source_message: None,
            };

            ProtocolError::InvalidMessage {
                string: s.to_owned(),
                cause,
            }
        })?;

        // Convert parsed tags to owned Tag structs
        let tags = parsed.tags.map(parse_tags_string);

        // Build the owned Message
        Message::with_tags(tags, parsed.prefix, parsed.command, parsed.params.to_vec()).map_err(
            |cause| ProtocolError::InvalidMessage {
                string: s.to_owned(),
                cause,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::Command;

    #[test]
    fn test_parse_simple_ping() {
        let msg: Message = "PING :server\r\n".parse().unwrap();
        assert!(matches!(msg.command, Command::PING(_, _)));
    }

    #[test]
    fn test_parse_server_pass_ts6() {
        let msg: Message = "PASS hunter2 TS 6 :001\r\n".parse().unwrap();
        match msg.command {
            Command::PassTs6 { password, sid } => {
                assert_eq!(password, "hunter2");
                assert_eq!(sid, "001");
            }
            other => panic!("expected PassTs6, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_privmsg() {
        let msg: Message = ":nick!user@host PRIVMSG #channel :Hello, world!\r\n"
            .parse()
            .unwrap();
        assert!(matches!(msg.command, Command::PRIVMSG(_, _)));
        assert!(msg.prefix.is_some());
    }

    #[test]
    fn test_parse_with_tags() {
        let msg: Message = "@time=2023-01-01T00:00:00Z;msgid=abc123 :nick PRIVMSG #ch :Hi\r\n"
            .parse()
            .unwrap();

        assert!(msg.tags.is_some());
        let tags = msg.tags.as_ref().unwrap();
        assert_eq!(tags.len(), 2);

        // Check tag values
        assert_eq!(msg.tag_value("time"), Some("2023-01-01T00:00:00Z"));
        assert_eq!(msg.tag_value("msgid"), Some("abc123"));
    }

    #[test]
    fn test_parse_escaped_tags() {
        // Test tag value escaping: \s = space, \: = semicolon
        let msg: Message = "@key=value\\swith\\sspace PING :test\r\n".parse().unwrap();
        assert_eq!(msg.tag_value("key"), Some("value with space"));
    }

    #[test]
    fn test_parse_empty_message() {
        let result: Result<Message, _> = "".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_numeric_response() {
        let msg: Message = ":server 001 nick :Welcome to IRC\r\n".parse().unwrap();
        assert!(matches!(msg.command, Command::Response(_, _)));
    }

    #[test]
    fn test_parse_encap() {
        let msg: Message = ":sid ENCAP * LOGIN AAAAB test\r\n".parse().unwrap();
        match msg.command {
            Command::ENCAP(target, subcommand, params) => {
                assert_eq!(target, "*");
                assert_eq!(subcommand, "LOGIN");
                assert_eq!(params, vec!["AAAAB".to_string(), "test".to_string()]);
            }
            other => panic!("expected ENCAP, got {other:?}"),
        }
    }

    #[test]
    fn test_parse_user_command() {
        let msg: Message = "USER guest 0 * :Real Name\r\n".parse().unwrap();
        assert!(matches!(msg.command, Command::USER(_, _, _)));
    }

    #[test]
    fn test_parse_join() {
        let msg: Message = "JOIN #channel\r\n".parse().unwrap();
        assert!(matches!(msg.command, Command::JOIN(_, _, _)));
    }

    #[test]
    fn test_parse_mode() {
        let msg: Message = "MODE #channel +o nick\r\n".parse().unwrap();
        assert!(matches!(msg.command, Command::ChannelMODE(_, _)));
    }

    #[test]
    fn test_intern_common_tags() {
        // Verify that common tags are interned (borrowed, not owned)
        let key = intern_tag_key("msgid");
        assert!(matches!(key, Cow::Borrowed(_)));

        let key = intern_tag_key("time");
        assert!(matches!(key, Cow::Borrowed(_)));

        let key = intern_tag_key("unknown-tag");
        assert!(matches!(key, Cow::Owned(_)));
    }
}
