//! Property-based tests for IRC message parsing.
//!
//! Uses proptest to generate random IRC components and verify that:
//! 1. Parsing never panics on well-formed input
//! 2. Serialized messages can be re-parsed (roundtrip)
//! 3. Parser invariants hold across random inputs
//!
//! Run with: `cargo test --features proptest`

use proptest::prelude::*;
use slirc_proto::{Command, Message, Prefix, Tag};

// =============================================================================
// STRATEGIES - Generators for valid IRC components
// =============================================================================

/// Valid IRC nickname: starts with letter or special char, followed by
/// letters, digits, or special chars. Max 9 chars per RFC 2812.
fn nickname_strategy() -> impl Strategy<Value = String> {
    // First char: letter or special [\]^_`{|}
    // Rest: letter, digit, hyphen, or special
    prop::string::string_regex("[a-zA-Z\\[\\]\\\\^_`{|}][a-zA-Z0-9\\-\\[\\]\\\\^_`{|}]{0,8}")
        .expect("valid regex")
}

/// Valid IRC username (ident): alphanumeric, no spaces or @ or !
fn username_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z][a-zA-Z0-9]{0,9}").expect("valid regex")
}

/// Valid hostname: simplified version
fn hostname_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z0-9]+(\\.[a-z0-9]+)*").expect("valid regex")
}

/// Valid IRC channel name: starts with # or &, followed by valid chars
fn channel_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[#&][a-zA-Z0-9_\\-]{1,49}").expect("valid regex")
}

/// Message text that doesn't contain CR/LF (which would break IRC protocol)
fn message_text_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[^\r\n\0]{0,400}").expect("valid regex")
}

/// "Dangerous" message text that tests edge cases in serialization.
/// These strings probe colon handling, space handling, and potential injection.
fn dangerous_message_text_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        // Edge cases for colon prefixing
        Just("".to_string()),           // Empty (needs colon prefix)
        Just(" ".to_string()),          // Single space (needs colon prefix)
        Just(":".to_string()),          // Single colon (needs colon prefix)
        Just("::".to_string()),         // Double colon
        Just(": trailing".to_string()), // Colon followed by space
        Just(":leading".to_string()),   // Colon at start
        // Multi-word messages that need colon prefix
        Just("hello world".to_string()),
        Just("param with spaces".to_string()),
        Just("multiple   spaces   here".to_string()),
        // Special characters that are valid in trailing
        Just("test;with;semicolons".to_string()),
        Just("backslash\\here".to_string()),
        Just("mixed :colon and space".to_string()),
        // Very long messages (near limits)
        Just("x".repeat(400)),
    ]
}

/// Tag key: alphanumeric with optional vendor prefix
fn tag_key_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z][a-zA-Z0-9\\-]{0,30}").expect("valid regex")
}

/// Tag value: no spaces, semicolons, NUL, CR, LF, or backslash (simplified)
fn tag_value_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-zA-Z0-9._\\-]{0,200}").expect("valid regex")
}

/// Generate a valid Prefix
fn prefix_strategy() -> impl Strategy<Value = Prefix> {
    prop_oneof![
        // Server name (contains dot)
        prop::string::string_regex("[a-z]+\\.[a-z]+\\.[a-z]+")
            .expect("valid regex")
            .prop_map(Prefix::ServerName),
        // User prefix: nick!user@host
        (
            nickname_strategy(),
            username_strategy(),
            hostname_strategy()
        )
            .prop_map(|(nick, user, host)| Prefix::Nickname(nick, user, host)),
    ]
}

/// Generate a valid Tag
fn tag_strategy() -> impl Strategy<Value = Tag> {
    (tag_key_strategy(), prop::option::of(tag_value_strategy()))
        .prop_map(|(key, value)| Tag(key.into(), value))
}

/// Generate a list of tags
fn tags_strategy() -> impl Strategy<Value = Option<Vec<Tag>>> {
    prop::option::of(prop::collection::vec(tag_strategy(), 0..5))
}

/// Generate simple commands that are easy to roundtrip
fn command_strategy() -> impl Strategy<Value = Command> {
    prop_oneof![
        // PRIVMSG - most common
        (channel_strategy(), message_text_strategy())
            .prop_map(|(target, text)| Command::PRIVMSG(target, text)),
        // NOTICE
        (channel_strategy(), message_text_strategy())
            .prop_map(|(target, text)| Command::NOTICE(target, text)),
        // NICK
        nickname_strategy().prop_map(Command::NICK),
        // JOIN (simple form)
        channel_strategy().prop_map(|chan| Command::JOIN(chan, None, None)),
        // PART
        (
            channel_strategy(),
            prop::option::of(message_text_strategy())
        )
            .prop_map(|(chan, msg)| Command::PART(chan, msg)),
        // PING
        hostname_strategy().prop_map(|server| Command::PING(server, None)),
        // PONG
        hostname_strategy().prop_map(|server| Command::PONG(server, None)),
        // QUIT
        prop::option::of(message_text_strategy()).prop_map(Command::QUIT),
        // AWAY
        prop::option::of(message_text_strategy()).prop_map(Command::AWAY),
        // TOPIC
        (
            channel_strategy(),
            prop::option::of(message_text_strategy())
        )
            .prop_map(|(chan, topic)| Command::TOPIC(chan, topic)),
        // KICK
        (
            channel_strategy(),
            nickname_strategy(),
            prop::option::of(message_text_strategy())
        )
            .prop_map(|(chan, nick, reason)| Command::KICK(chan, nick, reason)),
        // INVITE
        (nickname_strategy(), channel_strategy())
            .prop_map(|(nick, chan)| Command::INVITE(nick, chan)),
        // WHO
        prop::option::of(channel_strategy()).prop_map(|mask| Command::WHO(mask, None)),
        // WHOIS
        nickname_strategy().prop_map(|nick| Command::WHOIS(None, nick)),
    ]
}

/// Generate commands with "dangerous" edge-case text that tests serialization robustness.
/// These specifically probe colon prefixing, space handling, and empty values.
fn dangerous_command_strategy() -> impl Strategy<Value = Command> {
    prop_oneof![
        // PRIVMSG with dangerous text
        (channel_strategy(), dangerous_message_text_strategy())
            .prop_map(|(target, text)| Command::PRIVMSG(target, text)),
        // NOTICE with dangerous text
        (channel_strategy(), dangerous_message_text_strategy())
            .prop_map(|(target, text)| Command::NOTICE(target, text)),
        // PART with dangerous reason
        (
            channel_strategy(),
            prop::option::of(dangerous_message_text_strategy())
        )
            .prop_map(|(chan, msg)| Command::PART(chan, msg)),
        // QUIT with dangerous message
        prop::option::of(dangerous_message_text_strategy()).prop_map(Command::QUIT),
        // TOPIC with dangerous text
        (
            channel_strategy(),
            prop::option::of(dangerous_message_text_strategy())
        )
            .prop_map(|(chan, topic)| Command::TOPIC(chan, topic)),
        // KICK with dangerous reason
        (
            channel_strategy(),
            nickname_strategy(),
            prop::option::of(dangerous_message_text_strategy())
        )
            .prop_map(|(chan, nick, reason)| Command::KICK(chan, nick, reason)),
    ]
}

/// Generate a complete valid Message
fn message_strategy() -> impl Strategy<Value = Message> {
    (
        tags_strategy(),
        prop::option::of(prefix_strategy()),
        command_strategy(),
    )
        .prop_map(|(tags, prefix, command)| Message {
            tags,
            prefix,
            command,
        })
}

/// Generate a message with dangerous edge-case content
fn dangerous_message_strategy() -> impl Strategy<Value = Message> {
    (
        tags_strategy(),
        prop::option::of(prefix_strategy()),
        dangerous_command_strategy(),
    )
        .prop_map(|(tags, prefix, command)| Message {
            tags,
            prefix,
            command,
        })
}

// =============================================================================
// PROPERTY TESTS
// =============================================================================

proptest! {
    /// The fundamental roundtrip property: parse → serialize → parse = identity
    #[test]
    fn message_roundtrip(msg in message_strategy()) {
        // Serialize the message
        let serialized = msg.to_string();

        // Parse it back
        let parsed: Message = serialized.parse()
            .expect("Serialized message should be parseable");

        // Should be semantically equal
        prop_assert_eq!(&msg, &parsed,
            "Roundtrip failed for serialized: {}", serialized);
    }

    /// Prefix roundtrip: any valid prefix can be parsed and re-serialized
    #[test]
    fn prefix_roundtrip(prefix in prefix_strategy()) {
        let serialized = prefix.to_string();
        let parsed = Prefix::new_from_str(&serialized);
        prop_assert_eq!(&prefix, &parsed,
            "Prefix roundtrip failed for: {}", serialized);
    }

    /// Tags should serialize in a way that can be parsed back
    #[test]
    fn tag_in_message_roundtrip(
        key in tag_key_strategy(),
        value in prop::option::of(tag_value_strategy())
    ) {
        let tag = Tag(key.clone().into(), value.clone());
        let msg = Message {
            tags: Some(vec![tag]),
            prefix: None,
            command: Command::PING("test".to_string(), None),
        };

        let serialized = msg.to_string();
        let parsed: Message = serialized.parse()
            .expect("Tagged message should parse");

        // Verify tag is present
        let parsed_value = parsed.tag_value(&key);
        prop_assert_eq!(value.as_deref(), parsed_value,
            "Tag value mismatch for key '{}': expected {:?}, got {:?}",
            key, value, parsed_value);
    }

    /// PRIVMSG with arbitrary (valid) content should roundtrip
    #[test]
    fn privmsg_roundtrip(
        nick in nickname_strategy(),
        user in username_strategy(),
        host in hostname_strategy(),
        target in channel_strategy(),
        text in message_text_strategy()
    ) {
        let msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(nick, user, host)),
            command: Command::PRIVMSG(target, text),
        };

        let serialized = msg.to_string();
        let parsed: Message = serialized.parse()
            .expect("PRIVMSG should parse");

        prop_assert_eq!(msg, parsed);
    }

    /// Parsing should never panic on syntactically valid IRC lines
    #[test]
    fn parse_never_panics_on_valid_input(msg in message_strategy()) {
        let serialized = msg.to_string();
        // This should not panic, even if it returns an error
        let _ = serialized.parse::<Message>();
    }

    /// Nickname parser extracts correct nick from full prefix
    #[test]
    fn source_nickname_extraction(
        nick in nickname_strategy(),
        user in username_strategy(),
        host in hostname_strategy()
    ) {
        let msg = Message {
            tags: None,
            prefix: Some(Prefix::Nickname(nick.clone(), user, host)),
            command: Command::PING("test".to_string(), None),
        };

        prop_assert_eq!(msg.source_nickname(), Some(nick.as_str()));
    }
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

proptest! {
    /// Empty message text should be handled correctly
    #[test]
    fn empty_message_text_roundtrip(target in channel_strategy()) {
        let msg = Message {
            tags: None,
            prefix: None,
            command: Command::PRIVMSG(target, String::new()),
        };

        let serialized = msg.to_string();
        let parsed: Message = serialized.parse().expect("Should parse");
        prop_assert_eq!(msg, parsed);
    }

    /// Multiple tags should maintain order and values
    #[test]
    fn multiple_tags_roundtrip(tags in prop::collection::vec(tag_strategy(), 1..5)) {
        let msg = Message {
            tags: Some(tags.clone()),
            prefix: None,
            command: Command::PING("test".to_string(), None),
        };

        let serialized = msg.to_string();
        let parsed: Message = serialized.parse().expect("Should parse");

        // Verify all tags are present (order may not be preserved)
        let parsed_tags = parsed.tags.as_ref().expect("Tags should exist");
        prop_assert_eq!(tags.len(), parsed_tags.len());

        for tag in &tags {
            let found = parsed_tags.iter().any(|t| t.0 == tag.0 && t.1 == tag.1);
            prop_assert!(found, "Tag {:?} not found in parsed message", tag);
        }
    }
}

// =============================================================================
// DANGEROUS INPUT TESTS - Serialization Edge Cases
// =============================================================================

proptest! {
    /// Messages with dangerous edge-case content should roundtrip correctly.
    /// This tests colon prefixing, space handling, and empty values.
    #[test]
    fn dangerous_message_roundtrip(msg in dangerous_message_strategy()) {
        let serialized = msg.to_string();

        // The serialized form should end with CRLF and have no embedded CRLF
        prop_assert!(serialized.ends_with("\r\n"),
            "Serialized message should end with CRLF: {:?}", serialized);

        let without_trailing = &serialized[..serialized.len() - 2];
        let has_crlf_in_middle = without_trailing.contains("\r\n")
            || without_trailing.contains('\r')
            || without_trailing.contains('\n');
        prop_assert!(!has_crlf_in_middle,
            "Serialized message has embedded CR/LF: {:?}", serialized);

        // Parse it back
        let parsed: Message = serialized.parse()
            .expect("Dangerous message should be parseable");

        // Should be semantically equal
        prop_assert_eq!(&msg, &parsed,
            "Roundtrip failed for dangerous message. Serialized: {:?}", serialized);
    }

    /// Specifically test PRIVMSG with various dangerous text values
    #[test]
    fn privmsg_dangerous_text_roundtrip(
        target in channel_strategy(),
        text in dangerous_message_text_strategy()
    ) {
        let msg = Message {
            tags: None,
            prefix: None,
            command: Command::PRIVMSG(target, text),
        };

        let serialized = msg.to_string();
        let parsed: Message = serialized.parse()
            .expect("PRIVMSG with dangerous text should parse");

        prop_assert_eq!(msg, parsed,
            "PRIVMSG roundtrip failed. Serialized: {:?}", serialized);
    }

    /// Test that serialized wire format is always valid IRC protocol
    #[test]
    fn wire_format_validity(msg in dangerous_message_strategy()) {
        let serialized = msg.to_string();

        // Wire format rules:
        // 1. No NUL characters
        prop_assert!(!serialized.contains('\0'),
            "Wire format contains NUL: {:?}", serialized);

        // 2. Message should end with CRLF (added by Message::fmt)
        prop_assert!(serialized.ends_with("\r\n"),
            "Wire format should end with CRLF: {:?}", serialized);

        // 3. No bare CR or LF except at the very end
        let without_trailing = &serialized[..serialized.len() - 2];
        prop_assert!(!without_trailing.contains('\r') && !without_trailing.contains('\n'),
            "Wire format contains embedded CR or LF: {:?}", serialized);

        // 4. Should parse back successfully
        let _: Message = serialized.parse()
            .expect("Valid wire format should parse");
    }
}
